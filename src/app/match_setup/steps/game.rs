//! Step 1's rows: what each one turns, says and notes, and its keys.

use super::*;

/// Turn step 1's `row` one notch. Scoring is the settings' team mode,
/// the one Settings turns too; everything else is the match's own.
pub fn turn_game_row(
    row: GameRow,
    turn: Turn,
    config: &mut MatchConfig,
    settings: &mut GameSettings,
    beaches: &CustomBeaches,
) {
    match row {
        GameRow::Players => {
            let seats = crate::app::cycle::dial(config.seats, turn, 1, SEATS_RANGE);
            reseat(config, seats, beaches);
        }
        // The level of every AI seat: a row that said "for every AI seat"
        // and left the seats where they were would be a row that lies.
        GameRow::AiLevel => {
            config.ai_level = config.ai_level.cycled(turn);
            let level = config.ai_level;
            for controller in &mut config.controllers {
                if controller.ai().is_some() {
                    *controller = SeatController::Ai(level);
                }
            }
        }
        GameRow::Round => config.round = config.round.cycled(turn),
        GameRow::Posts => {
            config.posts = crate::app::cycle::dial(config.posts, turn, 1, POSTS_RANGE);
        }
        GameRow::Gulls => config.gulls = config.gulls.cycled(turn),
        // Every mode, whether the table fits it yet or not, as the lobby's
        // teams dial turns: the row says when it starts.
        GameRow::Scoring => settings.team_mode = settings.team_mode.cycled(turn),
        GameRow::Series => config.series = config.series.cycled(turn),
    }
}

/// Step 1's row as it reads: its name, its value with the dial's `< >`,
/// and the quiet aside after the value, if it has one.
pub fn game_row_text(
    tr: &crate::app::i18n::Tr,
    row: GameRow,
    config: &MatchConfig,
    settings: &GameSettings,
) -> (&'static str, String, &'static str) {
    let dial = |value: &str| format!("< {value} >");
    match row {
        GameRow::Players => (
            tr.match_players,
            dial(&config.seats.to_string()),
            tr.setup_players_aside,
        ),
        GameRow::AiLevel => (
            tr.match_ai_level,
            dial(tr.bot_levels[config.ai_level.index()]),
            tr.setup_ai_aside,
        ),
        GameRow::Round => (
            tr.match_round,
            dial(tr.round_names[config.round.index()]),
            "",
        ),
        GameRow::Posts => (tr.match_posts, dial(&config.posts.to_string()), ""),
        GameRow::Gulls => (
            tr.match_gulls,
            dial(tr.gull_names[config.gulls.index()]),
            "",
        ),
        GameRow::Scoring => (
            tr.setup_scoring,
            dial(&crate::app::teams::row_value(
                tr,
                settings.team_mode,
                config.seats,
                true,
            )),
            "",
        ),
        GameRow::Series => (
            tr.setup_series,
            dial(tr.mode_names[config.series.index()]),
            "",
        ),
    }
}

/// What step 1's `row` is for, said under the rows while the cursor is
/// on it. The players range is filled from [`SEATS_RANGE`].
pub fn game_row_note(tr: &crate::app::i18n::Tr, row: GameRow) -> String {
    let index = GameRow::ALL.iter().position(|r| *r == row).unwrap_or(0);
    fill(
        tr.setup_game_notes[index],
        &[
            ("a", &SEATS_RANGE.start().to_string()),
            ("b", &SEATS_RANGE.end().to_string()),
        ],
    )
}

/// Step 1: W/S choose a row, A/D turn it, Enter on to the map, Esc back
/// to the menu.
pub(super) fn game_keys(
    keys: &ButtonInput<KeyCode>,
    menu: &mut MatchMenu,
    config: &mut MatchConfig,
    settings: &mut GameSettings,
    beaches: &CustomBeaches,
    start: &mut MatchStart,
    pads: &mut SetupPads,
) {
    if keys.just_pressed(KeyCode::Escape) {
        start.next_screen.set(Screen::Menu);
        return;
    }
    if menu_ui::enter(keys) {
        open_map_step(menu, config, beaches);
        return;
    }
    menu.game_row = menu_ui::nav(keys, menu.game_row, GameRow::ALL.len());
    let Some(turn) = menu_ui::left_right(keys) else {
        return;
    };
    let row = GameRow::ALL[menu.game_row];
    turn_game_row(row, turn, config, settings, beaches);
    if row == GameRow::Players {
        // The people and the AI are now what the dials say.
        pads.seats.table_set_by_hand();
    }
}
