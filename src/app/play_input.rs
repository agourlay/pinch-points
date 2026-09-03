//! What the keys mean while a round is on: the phase-flow input systems.
//!
//! The cursor file answers "where is the cursor and how does it move";
//! this one answers "what does a press do to the round": place a post,
//! start the run, retry, advance the campaign, queue a versus action,
//! leave the results card. None of it moves a cursor, and two of the
//! systems never read one.

use crate::app::cursor::{Cursor, FLASH_SECS, keymap};
use crate::app::settings::{CommitScheme, GameSettings};
use crate::app::{Campaign, LoadLevel, PendingActions, Phase, PlacementDenied, Screen, Sim};
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
        // Backward wraps onto the end of the list, which is as locked as
        // anything ahead: the same ladder, the same refusal.
        let prev = (campaign.index + campaign.levels.len() - 1) % campaign.levels.len();
        if progress.unlocked(&campaign, prev) {
            campaign.index = prev;
            load.write(LoadLevel { keep_posts: false });
        } else {
            cursor.flash = FLASH_SECS;
            denied.write(PlacementDenied {
                player: 0,
                out_of_signposts: false,
            });
        }
    }
}

/// Puzzle running phase: R resets to setup (keeping placed posts).
///
/// Esc is the pause card's key, as it is in setup and in versus. This
/// used to toggle `Paused` on it as well, which the card then read as the
/// state to hand back on closing: Continue left the round frozen with no
/// card up, and the next Esc pair unfroze it.
pub fn running_input(
    keys: Res<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    mut load: MessageWriter<LoadLevel>,
) {
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
        // last level" and then puts you back on the first. The shipped
        // campaign ends with its last shipped stage too, rather than
        // walking on into the player's own levels behind it.
        if campaign.index + 1 == campaign.levels.len() || campaign.index + 1 == campaign.builtins {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::campaign::CampaignKind;
    use crate::app::progress::Progress;
    use crate::sim::{Level, campaign_levels};

    /// Three shipped stages, and one of the player's own behind them when
    /// asked for.
    fn campaign_with(index: usize, mine: bool) -> Campaign {
        let mut levels: Vec<Level> = campaign_levels().into_iter().take(3).collect();
        if mine {
            let mut mine = levels[0].clone();
            mine.name = "Mine".into();
            levels.push(mine);
        }
        Campaign {
            kind: CampaignKind::TidePool,
            levels,
            index,
            builtins: 3,
        }
    }

    fn campaign(index: usize) -> Campaign {
        campaign_with(index, true)
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

    /// The won card, with the list at `index`.
    fn won(index: usize) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.init_state::<Phase>();
        app.insert_resource(State::new(Screen::Puzzle));
        app.insert_resource(State::new(Phase::Won));
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.insert_resource(campaign(index));
        app.add_message::<LoadLevel>();
        app.add_systems(Update, done_input);
        app
    }

    fn loads(app: &App) -> usize {
        app.world()
            .resource::<Messages<LoadLevel>>()
            .iter_current_update_messages()
            .count()
    }

    /// The shipped campaign ends with its last shipped stage: Enter there
    /// goes to the menu rather than walking on into the player's own
    /// levels behind it, which are a shelf and not stage four.
    #[test]
    fn enter_after_the_last_shipped_stage_goes_to_the_menu() {
        let mut app = won(2);
        tap(&mut app, KeyCode::Enter);
        assert_eq!(loads(&app), 0, "nothing loaded");
        app.update();
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
        assert_eq!(
            app.world().resource::<Campaign>().index,
            2,
            "the list stays put"
        );
    }

    /// In the middle of the list Enter loads the next stage, and the last
    /// of the player's own levels ends the list the same way the campaign
    /// ends.
    #[test]
    fn enter_walks_the_list_and_stops_at_its_end() {
        let mut app = won(0);
        tap(&mut app, KeyCode::Enter);
        assert_eq!(loads(&app), 1, "the next stage is loaded");
        assert_eq!(app.world().resource::<Campaign>().index, 1);
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Puzzle
        );

        let mut app = won(3);
        tap(&mut app, KeyCode::Enter);
        assert_eq!(loads(&app), 0);
        app.update();
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
    }

    /// Browsing from setup honours the ladder both ways: P from the first
    /// stage wraps onto the end of the shipped list, which is as locked as
    /// any stage ahead, and is refused with the same flash; once the first
    /// stage is cleared N opens the second, and P walks back. (With a
    /// level of the player's own on the shelf the wrap lands on that, and
    /// those are never locked, so the list here is the shipped one.)
    #[test]
    fn browsing_from_setup_honours_the_ladder_both_ways() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Phase>();
        app.insert_resource(State::new(Phase::Setup));
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.insert_resource(GameSettings::default());
        app.init_resource::<Progress>();
        app.insert_resource(Sim(campaign_with(0, false).levels[0].board()));
        app.insert_resource(campaign_with(0, false));
        app.add_message::<LoadLevel>();
        app.add_message::<PlacementDenied>();
        app.add_systems(Update, setup_input);
        let mut cursor = Cursor::seated(0);
        (cursor.x, cursor.y) = (1, 1);
        app.world_mut().spawn((cursor, Transform::default()));
        let denials = |app: &App| {
            app.world()
                .resource::<Messages<PlacementDenied>>()
                .iter_current_update_messages()
                .count()
        };

        tap(&mut app, KeyCode::KeyP);
        assert_eq!(
            app.world().resource::<Campaign>().index,
            0,
            "the end is locked"
        );
        assert_eq!(denials(&app), 1, "and refused out loud");
        tap(&mut app, KeyCode::KeyN);
        assert_eq!(
            app.world().resource::<Campaign>().index,
            0,
            "so is the second"
        );
        assert_eq!(denials(&app), 1);

        let first = app.world().resource::<Campaign>().levels[0].name.clone();
        app.world_mut()
            .resource_mut::<Progress>()
            .mark(CampaignKind::TidePool, &first);
        tap(&mut app, KeyCode::KeyN);
        assert_eq!(
            app.world().resource::<Campaign>().index,
            1,
            "cleared, so open"
        );
        assert_eq!(loads(&app), 1);
        tap(&mut app, KeyCode::KeyP);
        assert_eq!(app.world().resource::<Campaign>().index, 0, "and back");
        assert_eq!(denials(&app), 0);
    }
}
