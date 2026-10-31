//! The three setup steps, driven the way a player drives them: keys into
//! the input system, and what each step then shows and starts.

use super::ui::{
    ASIDE_FONT, ASIDE_GAP, BADGE_FONT, BY_W, CARD_NAME_FONT, CARD_SUB_FONT, LABEL_GAP, LABEL_W,
    MAP_CARD_W, NAME_INSET, NAME_W, NOTE_FONT, OPEN_FONT, PILL_FONT, ROW_FONT, SEAT_HINT_FONT,
    SEATS_CARD_W, STATUS_FONT, STATUS_W, THUMB_W, VALUE_W, card_name, card_note, moved_note,
    tile_words,
};
use super::ui::{HEADING_FONT, TILE_W, slot_note, slot_words};
use super::*;
use crate::app::gamepad::{LastDevice, PadFamily};
use crate::app::i18n::metrics::{display_px, text_px};
use crate::app::settings::GameSettings;

/// A level with castles for `seats` players, as a shelf beach.
fn beach(name: &str, castles: &str) -> Beach {
    let text = format!(
        "name: {name}\nposts: 1\nkind: arena\ncrab: 0,1 R R common\nmap:\n\
         +-+-+-+\n|{castles}|\n+ + + +\n|. . .|\n+ + + +\n|. . .|\n+-+-+-+\n"
    );
    Beach::new(crate::sim::Level::parse(&text).expect("a level"))
}

/// Match setup in a headless app, on `step`, with the input system and
/// the resources it reads.
fn setup(config: MatchConfig, shelf: CustomBeaches, step: Step) -> App {
    let mut app = App::new();
    app.add_plugins(bevy::state::app::StatesPlugin);
    app.init_state::<Screen>();
    app.insert_resource(State::new(Screen::MatchSetup));
    app.add_message::<bevy::input::keyboard::KeyboardInput>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.insert_resource(MatchMenu {
        step,
        seed: 0xC0FFEE,
        ..MatchMenu::default()
    });
    app.insert_resource(config);
    app.insert_resource(shelf);
    app.init_resource::<crate::app::bot_seats::BotSeats>();
    app.init_resource::<crate::app::tournament::Tournament>();
    app.init_resource::<crate::app::gamepad::PadSeats>();
    app.init_resource::<LastDevice>();
    app.init_resource::<crate::app::gamepad::LastPad>();
    app.init_resource::<crate::app::gamepad::Bridged>();
    app.init_resource::<crate::app::keycaps::KeyCaps>();
    app.insert_resource(GameSettings::default());
    app.add_systems(Update, match_setup_input);
    app.update();
    app
}

/// A key pressed for one frame.
fn tap(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    app.update();
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release(key);
    keys.clear();
}

fn menu(app: &App) -> &MatchMenu {
    app.world().resource::<MatchMenu>()
}

fn config(app: &App) -> &MatchConfig {
    app.world().resource::<MatchConfig>()
}

/// Back and forth through the steps, every choice and every step's cursor
/// is where it was left: one config, and a cursor a step.
#[test]
fn going_back_and_forth_keeps_every_choice() {
    let mut app = setup(MatchConfig::default(), CustomBeaches::default(), Step::Game);
    // Step 1: four players, the round on its longest.
    tap(&mut app, KeyCode::KeyD);
    tap(&mut app, KeyCode::KeyD);
    tap(&mut app, KeyCode::KeyS);
    tap(&mut app, KeyCode::KeyS);
    tap(&mut app, KeyCode::KeyD);
    assert_eq!(config(&app).seats, 4);
    assert_eq!(config(&app).round, RoundLength::Long);
    tap(&mut app, KeyCode::Enter);
    assert_eq!(menu(&app).step, Step::Map);
    // Step 2: one card right of the pick, the open ocean's row below it.
    let picked = menu(&app).slot;
    tap(&mut app, KeyCode::KeyD);
    tap(&mut app, KeyCode::KeyS);
    let card = menu(&app).slot;
    assert_eq!(card, picked + 1 + CARD_COLUMNS);
    let map = config(&app).map;
    assert_eq!(
        Slot::Card(Card::Map(map)),
        map_slots(&CustomBeaches::default())[card].1
    );
    tap(&mut app, KeyCode::Escape);
    assert_eq!(menu(&app).step, Step::Game);
    assert_eq!(menu(&app).game_row, 2, "step 1's cursor where it was left");
    assert_eq!((config(&app).seats, config(&app).map), (4, map));
    tap(&mut app, KeyCode::Enter);
    assert_eq!(menu(&app).slot, card, "step 2 opens on the pick");
    tap(&mut app, KeyCode::Enter);
    assert_eq!(menu(&app).step, Step::Players);
    // Step 3: P3 to the AI, then back to the map and forward again.
    tap(&mut app, KeyCode::KeyS);
    tap(&mut app, KeyCode::KeyS);
    tap(&mut app, KeyCode::KeyD);
    assert_eq!(
        config(&app).controller(2),
        SeatController::Ai(BotLevel::Easy)
    );
    tap(&mut app, KeyCode::Escape);
    assert_eq!(menu(&app).step, Step::Map);
    tap(&mut app, KeyCode::Enter);
    assert_eq!(menu(&app).seat, 2, "step 3's cursor where it was left");
    assert_eq!(
        config(&app).controller(2),
        SeatController::Ai(BotLevel::Easy)
    );
    assert_eq!(config(&app).round, RoundLength::Long);
    assert!(!config(&app).armed, "nothing started on the way");
    // And Esc from step 1 is the menu.
    tap(&mut app, KeyCode::Escape);
    tap(&mut app, KeyCode::Escape);
    tap(&mut app, KeyCode::Escape);
    assert!(matches!(
        app.world().resource::<NextState<Screen>>(),
        NextState::Pending(Screen::Menu)
    ));
}

/// Step 1's dials: the players held to the beaches' range, the AI level
/// setting every AI seat (the row says "for every AI seat"), and the rest
/// stepping their own values.
#[test]
fn step_one_turns_its_dials() {
    let shelf = CustomBeaches::default();
    let mut settings = GameSettings::default();
    let mut config = MatchConfig::with_ai(3, 2);
    config.controllers[2] = SeatController::Bot;
    for _ in 0..10 {
        turn_game_row(
            GameRow::Players,
            Turn::Right,
            &mut config,
            &mut settings,
            &shelf,
        );
    }
    assert_eq!(config.seats, *SEATS_RANGE.end());
    assert!(
        holds(config.map, config.seats),
        "reseated onto a wide beach"
    );
    for _ in 0..10 {
        turn_game_row(
            GameRow::Players,
            Turn::Left,
            &mut config,
            &mut settings,
            &shelf,
        );
    }
    assert_eq!(config.seats, *SEATS_RANGE.start());
    turn_game_row(
        GameRow::AiLevel,
        Turn::Right,
        &mut config,
        &mut settings,
        &shelf,
    );
    assert_eq!(config.ai_level, BotLevel::Hard);
    assert_eq!(config.controller(1), SeatController::Ai(BotLevel::Hard));
    turn_game_row(
        GameRow::Posts,
        Turn::Right,
        &mut config,
        &mut settings,
        &shelf,
    );
    assert_eq!(config.posts, MAX_SIGNPOSTS_PER_PLAYER as u8 + 1);
    turn_game_row(
        GameRow::Series,
        Turn::Right,
        &mut config,
        &mut settings,
        &shelf,
    );
    assert!(config.series.is_series());
    turn_game_row(
        GameRow::Scoring,
        Turn::Right,
        &mut config,
        &mut settings,
        &shelf,
    );
    assert_eq!(settings.team_mode, crate::app::teams::TeamMode::Pairs);
    turn_game_row(
        GameRow::Gulls,
        Turn::Left,
        &mut config,
        &mut settings,
        &shelf,
    );
    assert_eq!(config.gulls, GullPressure::Calm);
}

/// Scoring keeps the rule it had in settings: a team mode is played only
/// where the table splits evenly, and the row says until when it is
/// free-for-all, the number filled from the rule.
#[test]
fn the_scoring_row_plays_teams_only_where_they_fit() {
    use crate::app::i18n::EN;
    use crate::app::teams::TeamMode;
    let settings = GameSettings {
        team_mode: TeamMode::Pairs,
        ..GameSettings::default()
    };
    let row = |seats| {
        let config = MatchConfig {
            seats,
            ..MatchConfig::default()
        };
        game_row_text(&EN, GameRow::Scoring, &config, &settings).1
    };
    assert!(row(4).contains(EN.team_modes[TeamMode::Pairs.index()]));
    let odd = row(5);
    assert!(
        odd.contains(EN.team_mode_names[TeamMode::Pairs.index()]),
        "{odd}"
    );
    assert!(odd.contains('6'), "free-for-all until six: {odd}");
    assert_eq!(TeamMode::Pairs.played_by(5), TeamMode::Solo);
}

/// Every row of step 1 fits the two cells that hold it, value and aside
/// together, in every language, on every stop of every dial.
#[test]
fn step_one_rows_fit_their_cells_in_every_language() {
    use crate::app::teams::TeamMode;
    for lang in crate::app::i18n::ALL_LANGS {
        let tr = lang.tr();
        for seats in SEATS_RANGE {
            for (mode, series) in TeamMode::ALL.into_iter().flat_map(|mode| {
                crate::app::tournament::SeriesLength::ALL.map(move |series| (mode, series))
            }) {
                for (level, (gulls, round)) in
                    BOT_LEVELS
                        .into_iter()
                        .zip(GullPressure::ALL.into_iter().zip([
                            RoundLength::Short,
                            RoundLength::Long,
                            RoundLength::Quick,
                        ]))
                {
                    let settings = GameSettings {
                        team_mode: mode,
                        ..GameSettings::default()
                    };
                    let config = MatchConfig {
                        seats,
                        ai_level: level,
                        gulls,
                        round,
                        series,
                        ..MatchConfig::default()
                    };
                    for row in GameRow::ALL {
                        let (label, value, aside) = game_row_text(tr, row, &config, &settings);
                        // With air before the value: a label that filled
                        // its cell ran into the dial's `<`.
                        let label_w = text_px(label, ROW_FONT) + LABEL_GAP;
                        assert!(label_w <= LABEL_W, "{lang:?} {row:?}: {label} {label_w}");
                        let wide = text_px(&value, ROW_FONT)
                            + match aside.is_empty() {
                                true => 0.0,
                                false => ASIDE_GAP + text_px(aside, ASIDE_FONT),
                            };
                        assert!(
                            wide <= VALUE_W,
                            "{lang:?} {row:?}: {value} {aside} is {wide}px of {VALUE_W}"
                        );
                    }
                }
            }
        }
    }
}

/// Everything the three cards say fits the place it is said, in every
/// language: the notes, the step names, the map cards' words and the
/// seats' cells. Measured in the faces and sizes they are set in.
#[test]
fn every_line_fits_its_place_in_every_language() {
    let shelf = CustomBeaches(vec![
        beach("Driftwood Arena", "0 . 1"),
        beach("Gap", "0 . 2"),
        beach("Driftwood Cove Number 00 Long Name", "0 . 1"),
        beach("一人あたりの矢印一人あたりの矢印一人あたりの矢印", "0 1 2"),
        beach(&"W".repeat(60), "0 . 1"),
    ]);
    let note_room = MAP_CARD_W - 2.0 * crate::app::menu_ui::CARD_PAD - 20.0;
    let seats_room = SEATS_CARD_W - 2.0 * crate::app::menu_ui::CARD_PAD - 20.0;
    let over = std::cell::RefCell::new(Vec::new());
    for lang in crate::app::i18n::ALL_LANGS {
        let tr = lang.tr();
        let long = |line: &str, px: f32, room: f32| {
            let wide = text_px(line, px);
            if wide > room {
                over.borrow_mut()
                    .push(format!("{lang:?}: {line:?} is {wide:.0}px of {room}"));
            }
        };
        // Step 1's note wraps on two lines at most.
        for row in GameRow::ALL {
            long(
                &game_row_note(tr, row),
                NOTE_FONT,
                1.8 * (LABEL_W + VALUE_W),
            );
        }
        // The step indicator: three discs, three names and the rules
        // between them, across the narrowest card.
        let names: f32 = tr
            .setup_steps
            .iter()
            .map(|name| 22.0 + 8.0 + display_px(name, 17.0))
            .sum();
        assert!(names + 2.0 * 40.0 <= LABEL_W + VALUE_W, "{lang:?}: {names}");
        // The lobby host's page: its heading, and the line under a beach
        // too big to travel, at the longest name a card gives.
        let heading = super::ui::host_map_title(tr, 6);
        let wide = display_px(&heading, super::ui::HOST_MAP_TITLE_FONT);
        assert!(wide <= MAP_CARD_W / 2.0, "{lang:?}: {heading}");
        let far = fill(tr.host_map_too_big, &[("m", &card_name(&"W".repeat(60)))]);
        long(&far, NOTE_FONT, note_room);
        // Step 2's section headings, each within the first two cards of
        // its row.
        for heading in tr.setup_sections {
            let wide = display_px(heading, HEADING_FONT);
            assert!(wide <= 2.0 * TILE_W, "{lang:?}: {heading} is {wide:.0}px");
        }
        for seats in SEATS_RANGE {
            let config = MatchConfig {
                seats,
                ..MatchConfig::default()
            };
            for (_, slot) in map_slots(&shelf)
                .into_iter()
                .chain([(MapSection::Yours, Slot::MakeOne)])
            {
                // On the couch's step 2, and on the lobby host's page.
                for online in [false, true] {
                    let words = slot_words(tr, slot, seats, &shelf, online);
                    long(&words.sub, CARD_SUB_FONT, THUMB_W);
                    long(words.badge, BADGE_FONT, THUMB_W - 40.0);
                    long(&words.name, CARD_NAME_FONT, THUMB_W - NAME_INSET);
                    if let Some(pill) = &words.pill {
                        let wide = display_px(pill, PILL_FONT) + 24.0;
                        assert!(wide <= THUMB_W, "{lang:?}: {pill}");
                    }
                    long(
                        &slot_note(tr, slot, &config, &shelf, online),
                        NOTE_FONT,
                        note_room,
                    );
                }
                let Slot::Card(card) = slot else {
                    continue;
                };
                // The pick moved off by the seat count, said on step 2
                // and on step 3's hint line.
                let mut moved = MatchConfig {
                    seats: 2,
                    ..MatchConfig::default()
                };
                pick_card(&mut moved, card, &shelf);
                reseat(&mut moved, seats, &shelf);
                if let Some(note) = moved_note(tr, &moved, &shelf) {
                    long(&note, NOTE_FONT, note_room);
                    long(&note, SEAT_HINT_FONT, seats_room);
                }
            }
        }
        // Step 3's cells, the dial's `< >` included where it shows.
        let by_room = BY_W - 40.0;
        for by in [
            PlayedBy::Keys(0),
            PlayedBy::Keys(1),
            PlayedBy::Pad {
                number: 6,
                joined: false,
            },
            PlayedBy::Pad {
                number: 6,
                joined: true,
            },
            PlayedBy::Ai(BotLevel::Normal),
            PlayedBy::Ai(BotLevel::Hard),
            PlayedBy::Ai(BotLevel::Easy),
            PlayedBy::Bot,
        ] {
            long(
                &format!("< {} >", played_by_text(tr, by, PadFamily::Xbox)),
                ROW_FONT,
                by_room,
            );
            long(status_text(tr, by), STATUS_FONT, STATUS_W);
        }
        // An open seat names the pad's Start, longest as "Options".
        for (keys, family) in [false, true]
            .into_iter()
            .flat_map(|keys| PadFamily::ALL.map(|family| (keys, family)))
        {
            long(
                &played_by_text(tr, PlayedBy::Open { keys }, family),
                OPEN_FONT,
                by_room,
            );
            long(
                status_text(tr, PlayedBy::Open { keys }),
                STATUS_FONT,
                STATUS_W,
            );
        }
        for device in [LastDevice::Keyboard, LastDevice::PAD] {
            long(&unnamed(tr, device), STATUS_FONT, NAME_W);
            long(&seat_hint(tr, device, 1), SEAT_HINT_FONT, seats_room);
        }
        // The line about teams, at the widest names there are, for every
        // seat and either hands, P2 pinned or not.
        for name in ["W".repeat(12), "あいうえおかきくけこさし".to_string()] {
            for mode in crate::app::teams::TeamMode::ALL {
                for (seats, ai) in [(4, 0), (5, 0), (6, 0)] {
                    let config = MatchConfig::with_ai(seats, ai);
                    let settings = GameSettings {
                        team_mode: mode,
                        names: std::array::from_fn(|_| name.clone()),
                        ..GameSettings::default()
                    };
                    for seat in 0..seats {
                        for device in [LastDevice::Keyboard, LastDevice::PAD] {
                            for pin in [
                                None,
                                Some(P2Pin::Keys),
                                Some(P2Pin::Pad(3)),
                                Some(P2Pin::KeptForKeys),
                                Some(P2Pin::KeptForPad(3)),
                            ] {
                                let said =
                                    partner_line(tr, device, (&config, &settings), seat, pin);
                                long(said.as_deref().unwrap_or(""), SEAT_HINT_FONT, seats_room);
                            }
                        }
                    }
                }
            }
        }
        let all_open: Vec<u8> = (1..MAX_PLAYERS as u8).collect();
        let (line, warns) = seats_line(tr, &all_open, BotLevel::Normal, (true, PadFamily::Xbox));
        assert!(warns);
        long(&line, ROW_FONT - 2.0, seats_room);
        for family in PadFamily::ALL {
            long(
                &seats_line(tr, &[], BotLevel::Normal, (true, family)).0,
                ROW_FONT - 2.0,
                seats_room,
            );
        }
    }
    let over = over.into_inner();
    assert!(over.is_empty(), "too long:\n{}", over.join("\n"));
}

/// A handmade beach's name is cut short with "…" where it would run past
/// its card, in every script, and kept whole where it fits; measured in
/// the shipped faces, so the widths the cut counts by are theirs.
#[test]
fn a_long_beach_name_is_cut_to_its_card() {
    let room = THUMB_W - NAME_INSET;
    for name in [
        "Driftwood Cove Number 00 Long Name",
        // Kana and kanji the Japanese face's subset carries.
        "一人あたりの矢印一人あたりの矢印一人",
        "Plage du Phare à l'Écume Très Longue",
        "Пляж с очень длинным названием тут",
        &"M".repeat(80),
        "ABC 一人あたり DEF 矢印 GHIJKLMNOPQRS",
    ] {
        let shown = card_name(name);
        let wide = text_px(&shown, CARD_NAME_FONT);
        assert!(wide <= room, "{shown:?} is {wide:.0}px of {room}");
        assert!(shown.ends_with('…'), "{shown:?}");
        // Cut where the shipped faces say it must be: the longest start
        // that fits with its "…", spaces before the "…" let go.
        let chars: Vec<char> = name.chars().collect();
        let fits = (0..=chars.len())
            .rev()
            .map(|n| chars[..n].iter().collect::<String>())
            .find(|start| text_px(&format!("{start}…"), CARD_NAME_FONT) <= room)
            .expect("an empty start fits");
        assert_eq!(shown, format!("{}…", fits.trim_end()), "{name}");
    }
    for name in ["Driftwood Arena", "一人あたりの矢印", "Gap"] {
        assert_eq!(card_name(name), name, "fits, whole");
    }
}

/// A map that cannot seat the table is greyed, not hidden: the built-in
/// four-castle beaches past four, a handmade beach past its castles, and
/// one whose castles skip a number at any table. Picking a greyed card
/// changes nothing; the Random card seats everyone.
#[test]
fn map_cards_grey_what_cannot_seat_the_table() {
    use crate::app::i18n::EN;
    let shelf = CustomBeaches(vec![beach("Pair", "0 . 1"), beach("Gap", "0 . 2")]);
    let all = cards(&shelf);
    assert_eq!(
        all.len(),
        MapChoice::ALL.len() - 1 + 2 + 1,
        "every built-in map, the shelf, Random"
    );
    assert_eq!(all.last(), Some(&Card::Random));
    for seats in SEATS_RANGE {
        for card in &all {
            let fits = card_fits(*card, seats, &shelf);
            let want = match card {
                Card::Map(map) => holds(*map, seats),
                Card::Shelf(0) => seats <= 2,
                Card::Shelf(_) => false,
                Card::Random => true,
            };
            assert_eq!(fits, want, "{card:?} at {seats}");
            assert_eq!(
                tile_words(&EN, *card, seats, &shelf, false).pill.is_some(),
                !fits,
                "{card:?} at {seats}: greyed, and says so"
            );
        }
    }
    let words = tile_words(&EN, Card::Map(MapChoice::Classic), 5, &shelf, false);
    assert_eq!(words.pill.as_deref(), Some("seats 4, not 5"));
    // A handmade versus map names the tables it is built for, odd ones
    // never among them, and the note says what to do.
    let words = tile_words(&EN, Card::Map(MapChoice::RoundTable), 3, &shelf, false);
    assert_eq!(words.pill.as_deref(), Some("seats 2/4/6, not 3"));
    assert!(words.sub.contains("2/4/6"), "{}", words.sub);
    // Every map with no edges says so on its card, whatever its tables,
    // and only those do.
    for map in MapChoice::ALL {
        let words = tile_words(&EN, Card::Map(map), 2, &shelf, false);
        assert_eq!(
            words.sub.contains("no edges"),
            map.wraps(),
            "{map:?}: {}",
            words.sub
        );
    }
    let three = MatchConfig {
        seats: 3,
        ..MatchConfig::default()
    };
    let note = card_note(&EN, Card::Map(MapChoice::Duel), &three, &shelf, false);
    assert!(
        note.contains("Duel") && note.contains("for 2 players"),
        "{note}"
    );
    assert!(!card_fits(Card::Map(MapChoice::Duel), 4, &shelf));
    assert!(card_fits(Card::Map(MapChoice::Surprise), 5, &shelf));
    let mut config = MatchConfig {
        seats: 5,
        map: MapChoice::GenLarge,
        ..MatchConfig::default()
    };
    pick_card(&mut config, Card::Map(MapChoice::Classic), &shelf);
    assert_eq!(
        config.map,
        MapChoice::GenLarge,
        "a greyed card is not picked"
    );
    config.seats = 2;
    pick_card(&mut config, Card::Shelf(0), &shelf);
    assert_eq!((config.map, config.custom), (MapChoice::Custom, 0));
    assert_eq!(picked_card(&config, &shelf), Card::Shelf(0));
    pick_card(&mut config, Card::Random, &shelf);
    assert_eq!(picked_card(&config, &shelf), Card::Random);
}

/// A shelf beach with a `seats:` line is labelled, greyed and drawn by the
/// tables that line leaves it, the list `Level::fits` reads: four castles
/// and `seats: 4` seats four and only four, says so, and its card has a
/// picture at a table of two (drawn for four); `seats: 2 4` says "2/4",
/// never "2-4". Counted off the castles, the first read "2-4 players"
/// while greyed for two, and was drawn for two, which gave no picture.
#[test]
fn a_shelf_beachs_seats_line_labels_and_draws_its_card() {
    use crate::app::i18n::EN;
    let listed = |name: &str, seats: &str| {
        let text = format!(
            "name: {name}\nposts: 1\nkind: arena\nseats: {seats}\ncrab: 0,1 R R common\nmap:\n\
             +-+-+-+-+\n|0 . . 1|\n+ + + + +\n|. . . .|\n+ + + + +\n|2 . . 3|\n+-+-+-+-+\n"
        );
        Beach::new(crate::sim::Level::parse(&text).expect("a level"))
    };
    let shelf = CustomBeaches(vec![listed("Four", "4"), listed("Even", "2 4")]);
    let four = Card::Shelf(0);
    let even = Card::Shelf(1);
    assert_eq!(card_seats(four, &shelf), Some((4, 4)));
    assert_eq!(card_tables(even, &shelf), vec![2, 4]);
    for seats in SEATS_RANGE {
        assert_eq!(card_fits(four, seats, &shelf), seats == 4, "{seats}");
        assert_eq!(card_fits(even, seats, &shelf), seats == 2 || seats == 4);
    }
    let words = tile_words(&EN, four, 2, &shelf, false);
    assert_eq!(words.pill.as_deref(), Some("seats 4, not 2"));
    assert!(words.sub.ends_with("· 4 players"), "{}", words.sub);
    let words = tile_words(&EN, even, 3, &shelf, false);
    assert_eq!(words.pill.as_deref(), Some("seats 2/4, not 3"));
    assert!(words.sub.ends_with("· 2/4 players"), "{}", words.sub);
    let two = MatchConfig::default();
    let note = card_note(&EN, four, &two, &shelf, false);
    assert!(note.contains("built for 4 players"), "{note}");
    let shown = card_config(&two, four, &shelf).expect("a picture");
    assert_eq!((shown.seats, shown.map), (4, MapChoice::Custom));
    assert_eq!(
        shelf
            .fitting(4)
            .into_iter()
            .nth(shown.custom)
            .map(|b| &b.level.name),
        Some(&"Four".to_string())
    );
}

/// Every map has its line, in every language: `Custom` its own, the
/// player's beach, where it used to read past the end of the built-in
/// maps' lines and panic.
#[test]
fn every_map_says_what_it_is() {
    for lang in crate::app::i18n::ALL_LANGS {
        let tr = lang.tr();
        for map in MapChoice::ALL {
            assert!(!map_about(tr, map).is_empty(), "{lang:?}: {map:?}");
        }
        assert_eq!(map_about(tr, MapChoice::Custom), tr.setup_custom_about);
    }
}

/// Step 3 with the map moved by the seat count says so on a line of its
/// own, and still says what A/D does under it: the note used to take the
/// hint's place for the whole visit.
#[test]
fn step_three_says_the_map_moved_and_what_a_d_does() {
    use crate::app::i18n::EN;
    let shelf = CustomBeaches::default();
    let mut moved = MatchConfig {
        seats: 4,
        ..MatchConfig::default()
    };
    pick_card(&mut moved, Card::Map(MapChoice::GenSmall), &shelf);
    reseat(&mut moved, 5, &shelf);
    let note = moved_note(&EN, &moved, &shelf).expect("the map moved");
    let mut app = App::new();
    app.insert_resource(MatchMenu {
        step: Step::Players,
        seat: 1,
        ..MatchMenu::default()
    });
    app.insert_resource(moved);
    app.insert_resource(shelf);
    app.insert_resource(GameSettings::default());
    app.init_resource::<LastDevice>();
    app.init_resource::<crate::app::gamepad::LastPad>();
    app.insert_resource(crate::app::art::Art::blank());
    app.init_resource::<crate::app::gamepad::PadSeats>();
    app.add_systems(Update, (update_match_ui, paint_players_step).chain());
    app.update();
    app.update();
    let said = |app: &mut App| -> Vec<String> {
        let world = app.world_mut();
        let mut lines: Vec<String> = world
            .query_filtered::<(&Text, &Node), With<ui::SeatMoved>>()
            .iter(world)
            .filter(|(_, node)| node.display != Display::None)
            .map(|(text, _)| text.0.clone())
            .collect();
        lines.extend(
            world
                .query_filtered::<&Text, With<ui::SeatHint>>()
                .iter(world)
                .map(|text| text.0.clone()),
        );
        lines
    };
    assert_eq!(
        said(&mut app),
        vec![note, seat_hint(&EN, LastDevice::Keyboard, 1)]
    );
    // Back on a table the pick seats, only the hint.
    reseat(
        &mut app.world_mut().resource_mut::<MatchConfig>(),
        4,
        &CustomBeaches::default(),
    );
    app.update();
    assert_eq!(
        said(&mut app),
        vec![seat_hint(&EN, LastDevice::Keyboard, 1)]
    );
}

/// The cursor can rest on a greyed card, so its line can say why, but
/// Enter does not go on from it, and the pick stays where it was.
#[test]
fn enter_does_not_go_on_from_a_greyed_card() {
    // Gull Road is the first card that seats five: every card before it
    // is greyed at this table.
    let mut app = setup(
        MatchConfig {
            seats: 5,
            map: MapChoice::GullRoad,
            ..MatchConfig::default()
        },
        CustomBeaches::default(),
        Step::Map,
    );
    let gull_road = map_slots(&CustomBeaches::default())
        .iter()
        .position(|(_, slot)| *slot == Slot::Card(Card::Map(MapChoice::GullRoad)))
        .expect("Gull Road's card");
    app.world_mut().resource_mut::<MatchMenu>().slot = gull_road;
    for _ in 0..gull_road {
        tap(&mut app, KeyCode::KeyA);
    }
    assert_eq!(menu(&app).slot, 0, "resting on the classic arena");
    assert_eq!(config(&app).map, MapChoice::GullRoad, "still the pick");
    tap(&mut app, KeyCode::Enter);
    assert_eq!(menu(&app).step, Step::Map, "Enter did not go on");
    let note = card_note(
        &crate::app::i18n::EN,
        Card::Map(MapChoice::Classic),
        config(&app),
        &CustomBeaches::default(),
        false,
    );
    assert!(note.contains('4') && note.contains("Game"), "{note}");
    for _ in 0..gull_road {
        tap(&mut app, KeyCode::KeyD);
    }
    tap(&mut app, KeyCode::Enter);
    assert_eq!(menu(&app).step, Step::Players, "on from Gull Road");
}

/// The Random card draws among every map that seats the table but the
/// 20x13, which is only ever chosen, the player's own handmade ones
/// included, and writes the one drawn into the
/// config: the round is built, recorded and sent as that concrete map,
/// and the next round (a series' or a rematch's) draws again.
#[test]
fn random_draws_every_round_among_the_maps_that_seat_the_table() {
    let shelf = CustomBeaches(vec![beach("Pair", "0 . 1"), beach("Trio", "0 1 2")]);
    for seats in SEATS_RANGE {
        let maps = maps_that_seat(seats, &shelf);
        let mut drawn = std::collections::BTreeSet::new();
        let mut config = MatchConfig {
            seats,
            pick: Pick::Random,
            ..MatchConfig::default()
        };
        for roll in 0..64u64 {
            map_for_next_round(&mut config, &shelf, roll);
            assert_eq!(
                config.pick,
                Pick::Random,
                "still random for the round after"
            );
            let played = local_board(&config, &shelf, roll);
            match config.map {
                MapChoice::Custom => {
                    let beach = &shelf.fitting(seats)[config.custom];
                    assert_eq!(played.width(), beach.level.board().width());
                }
                // Chosen, never dealt: a pair drew 130 tiles each.
                MapChoice::GenXl => panic!("Random drew the 20x13 at {seats}"),
                // A way of playing, not a map to draw.
                MapChoice::Surprise => panic!("Random drew Surprise at {seats}"),
                map @ (MapChoice::Classic
                | MapChoice::GenSmall
                | MapChoice::GenClassic
                | MapChoice::GenLarge
                | MapChoice::GenOcean
                | MapChoice::TheGates
                | MapChoice::TheMoat
                | MapChoice::LogJam
                | MapChoice::ThreeLanes
                | MapChoice::RoundTable
                | MapChoice::RoundTheWorld
                | MapChoice::Duel
                | MapChoice::TheFountain
                | MapChoice::FourDoors
                | MapChoice::GullRoad
                | MapChoice::BackToBack) => {
                    assert!(holds(map, seats), "{map:?} at {seats}");
                    assert_eq!(played.width(), versus_board(map, roll, seats).width());
                }
            }
            drawn.insert(format!("{:?}{}", config.map, config.custom));
        }
        assert_eq!(drawn.len(), maps.len(), "every map that seats {seats}");
    }
    assert!(maps_that_seat(2, &shelf).contains(&(MapChoice::Custom, 1)));
    assert!(maps_that_seat(3, &shelf).contains(&(MapChoice::Custom, 0)));
    assert!(
        !maps_that_seat(5, &shelf)
            .iter()
            .any(|(map, _)| *map == MapChoice::Custom)
    );
}

/// Start match under the Random card draws the map there and then, and the
/// launched config names it; under any other card, the match is played on
/// the seed the card's picture was drawn with.
#[test]
fn start_match_draws_random_and_keeps_the_previewed_seed() {
    let shelf = CustomBeaches::default();
    let mut random = MatchConfig {
        seats: 3,
        pick: Pick::Random,
        map: MapChoice::GenXl,
        ..MatchConfig::default()
    };
    seat_the_match(&mut random, &[], 77, &shelf, 1);
    assert_eq!(random.seed, None, "a fresh seed for a drawn map");
    assert_eq!(
        (random.map, random.custom),
        maps_that_seat(3, &shelf)[1],
        "the draw the roll asked for"
    );
    let mut picked = MatchConfig {
        seats: 3,
        map: MapChoice::GenLarge,
        ..MatchConfig::default()
    };
    seat_the_match(&mut picked, &[2], 77, &shelf, 1);
    assert_eq!(picked.seed, Some(77));
    assert_eq!(picked.controller(2), SeatController::Ai(picked.ai_level));
    // And through the keys: Enter on step 3 starts it on the card's seed.
    let mut app = setup(
        MatchConfig {
            map: MapChoice::GenLarge,
            ..MatchConfig::default()
        },
        shelf,
        Step::Players,
    );
    tap(&mut app, KeyCode::Enter);
    let launched = config(&app);
    assert!(launched.armed);
    assert_eq!(launched.seed, Some(menu(&app).seed));
}

/// The AI put in an open seat at the start plays it for that match only:
/// back on the setup steps the seat is open again, so P2's keys take it,
/// as they could before.
#[test]
fn an_open_seat_gets_the_ai_for_that_match_only() {
    use bevy::ecs::system::RunSystemOnce;
    let mut app = setup(
        MatchConfig::default(),
        CustomBeaches::default(),
        Step::Players,
    );
    tap(&mut app, KeyCode::Enter);
    assert!(config(&app).armed);
    assert_eq!(
        config(&app).controller(1),
        SeatController::Ai(BotLevel::Normal)
    );
    app.world_mut()
        .run_system_once(enter_match_setup)
        .expect("runs");
    assert_eq!(
        config(&app).controller(1),
        SeatController::Local,
        "open again"
    );
    tap(&mut app, KeyCode::KeyI);
    assert!(
        app.world()
            .resource::<crate::app::gamepad::PadSeats>()
            .p2_at_keys()
    );
    let settings = GameSettings::default();
    assert_eq!(
        played_by(config(&app), 1, &[], true, &settings),
        PlayedBy::Keys(1),
        "P2's keys take it"
    );
    // A seat set to the AI by hand stays the AI's.
    let mut set = MatchConfig::with_ai(3, 1);
    seat_the_match(&mut set, &[1], 5, &CustomBeaches::default(), 0);
    settle(&mut set, &CustomBeaches::default());
    assert_eq!(set.controller(1), SeatController::Local);
    assert_eq!(set.controller(2), SeatController::Ai(BotLevel::Normal));
}

/// Start match at a table with a bot seat waits behind the doorway card,
/// and nothing is set for the match until it goes: backed out of with
/// Esc, the open seat is still open (not the AI's), the Random card has
/// drawn nothing, and no seed was taken. It used to fill the open seat
/// and draw the map first, and the cancel left both.
#[test]
fn a_doorway_card_backed_out_of_leaves_the_table_as_it_was() {
    let mut table = MatchConfig {
        seats: 3,
        map: MapChoice::Duel,
        pick: Pick::Random,
        ..MatchConfig::default()
    };
    table.controllers[2] = SeatController::Bot;
    let mut app = setup(table, CustomBeaches::default(), Step::Players);
    app.init_resource::<Clipboard>();
    app.init_resource::<crate::app::keycaps::KeyCaps>();
    app.init_resource::<crate::app::cursor::LiveSeatKeys>();
    app.add_systems(
        Update,
        crate::app::bot_seats::door_input.after(match_setup_input),
    );
    let as_set = |app: &App| {
        let config = config(app);
        (
            config.controllers[..3].to_vec(),
            config.filled_at_start,
            config.map,
            config.seed,
            config.armed,
        )
    };
    let before = as_set(&app);
    assert!(
        !maps_that_seat(3, &CustomBeaches::default())
            .iter()
            .any(|(map, _)| *map == MapChoice::Duel)
    );
    tap(&mut app, KeyCode::Enter);
    let bots = app.world().resource::<crate::app::bot_seats::BotSeats>();
    assert!(bots.door_for.card_up(), "waiting for the bot");
    assert_eq!(as_set(&app), before, "nothing set while the card waits");
    tap(&mut app, KeyCode::Escape);
    let bots = app.world().resource::<crate::app::bot_seats::BotSeats>();
    assert!(!bots.door_for.card_up(), "backed out");
    assert_eq!(as_set(&app), before, "and the table as step 3 showed it");
}

/// The seats the AI filled at the last start open again on the way back
/// in, at whatever level the AI plays them now: matched against the
/// level, one turned since left the seat to the AI for good. A seat set
/// to a bot since keeps it.
#[test]
fn the_seats_filled_at_start_open_again_whatever_the_level() {
    let shelf = CustomBeaches::default();
    let mut set = MatchConfig {
        seats: 4,
        ..MatchConfig::default()
    };
    seat_the_match(&mut set, &[1, 2, 3], 5, &shelf, 0);
    assert_eq!(set.controller(1), SeatController::Ai(BotLevel::Normal));
    set.ai_level = BotLevel::Hard;
    set.controllers[3] = SeatController::Bot;
    settle(&mut set, &shelf);
    assert_eq!(set.controller(1), SeatController::Local);
    assert_eq!(set.controller(2), SeatController::Local);
    assert_eq!(set.controller(3), SeatController::Bot);
}

/// The picture on a card is the board its match is played on: what step
/// 2 draws the card from (its config and the setup's seed) against what
/// the keys launch, Enter on the card and Enter on step 3, tick for tick
/// the same beach as the round's first.
#[test]
fn the_preview_is_the_board_played() {
    let shelf = || CustomBeaches(vec![beach("Pair", "0 . 1")]);
    for (index, (_, slot)) in map_slots(&shelf()).into_iter().enumerate() {
        let Slot::Card(card) = slot else {
            continue;
        };
        if card == Card::Random || card == Card::Map(MapChoice::Surprise) {
            continue;
        }
        // At the smallest table the card seats.
        let seats = match card {
            Card::Map(map) => map.tables().map_or(2, |tables| tables[0]),
            Card::Shelf(_) | Card::Random => 2,
        };
        let mut app = setup(
            MatchConfig {
                seats,
                ..MatchConfig::default()
            },
            shelf(),
            Step::Map,
        );
        app.world_mut().resource_mut::<MatchMenu>().slot = index;
        let shown = card_config(config(&app), card, &shelf()).expect("a beach to show");
        let pictured = preview_board(&shown, &shelf(), menu(&app).seed);
        tap(&mut app, KeyCode::Enter);
        assert_eq!(menu(&app).step, Step::Players, "{card:?}");
        tap(&mut app, KeyCode::Enter);
        let launched = config(&app);
        assert!(launched.armed, "{card:?}");
        let seed = launched.seed.expect("the previewed seed");
        assert_eq!(seed, menu(&app).seed);
        let mut played = local_board(launched, &shelf(), seed);
        for _ in 0..PREVIEW_TICKS {
            played.tick_idle();
        }
        assert_eq!(played.state_hash(), pictured.state_hash(), "{card:?}");
    }
}

/// Step 3's A/D steps a seat through a person, the AI at each level and a
/// bot, and never turns player 1, who is working the menu.
#[test]
fn step_three_turns_a_seat_but_never_player_one() {
    let mut config = MatchConfig {
        seats: 3,
        ..MatchConfig::default()
    };
    let mut seen = Vec::new();
    for _ in 0..SEAT_KINDS.len() + 1 {
        seen.push(config.controller(2));
        turn_seat(&mut config, 2, Turn::Right);
    }
    assert_eq!(seen[..SEAT_KINDS.len()], SEAT_KINDS);
    assert_eq!(
        seen[SEAT_KINDS.len()],
        SeatController::Bot,
        "held at the end"
    );
    turn_seat(&mut config, 0, Turn::Right);
    assert_eq!(config.controller(0), SeatController::Local);
    // And the hint under the seats says A/D only where it turns one.
    let en = &crate::app::i18n::EN;
    assert!(seat_hint(en, LastDevice::Keyboard, 0).is_empty());
    assert!(seat_hint(en, LastDevice::Keyboard, 2).contains("A/D"));
    assert_eq!(
        config.controller(1),
        SeatController::Local,
        "one seat at a time"
    );
    assert_eq!(bot_seat_list(&config), vec![2]);
}

/// Naming a seat on step 3: Tab opens it, what is typed lands in the
/// name, Enter hands the keyboard back and does not start the match, and
/// the next Enter does.
#[test]
fn typing_renames_a_seat_without_starting_the_match() {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput};
    let mut app = setup(
        MatchConfig::with_ai(2, 1),
        CustomBeaches::default(),
        Step::Players,
    );
    let type_char = |app: &mut App, ch: &str| {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyB,
            logical_key: Key::Character(ch.into()),
            state: ButtonState::Pressed,
            text: Some(ch.into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    };
    tap(&mut app, KeyCode::KeyS);
    tap(&mut app, KeyCode::Tab);
    assert_eq!(menu(&app).naming, Some(1), "the seat under the cursor");
    type_char(&mut app, "B");
    type_char(&mut app, "o");
    assert_eq!(app.world().resource::<GameSettings>().names[1], "Bo");
    tap(&mut app, KeyCode::Enter);
    assert_eq!(menu(&app).naming, None);
    assert!(!config(&app).armed, "the Enter that kept the name");
    tap(&mut app, KeyCode::Enter);
    assert!(config(&app).armed, "and the next one starts the match");
}

/// The seat count moves the map off the player's pick only when the pick
/// cannot seat the table, to the closest map that can, and step 2 says
/// so naming the pick; the pick comes back, and the note goes, the moment
/// the table fits it again. A pick that seats the table is kept, whatever
/// beach the table would have been dealt.
#[test]
fn the_map_follows_the_pick_and_says_when_it_cannot() {
    use crate::app::i18n::EN;
    let shelf = CustomBeaches::default();
    let mut app = setup(
        MatchConfig {
            map: MapChoice::GenSmall,
            ..MatchConfig::default()
        },
        shelf,
        Step::Game,
    );
    let note = |app: &App| {
        card_note(
            &EN,
            picked_card(config(app), &CustomBeaches::default()),
            config(app),
            &CustomBeaches::default(),
            false,
        )
    };
    tap(&mut app, KeyCode::KeyD);
    assert_eq!(
        (config(&app).seats, config(&app).map),
        (3, MapChoice::GenSmall)
    );
    assert!(!note(&app).contains("does not seat"), "{}", note(&app));
    for _ in 0..2 {
        tap(&mut app, KeyCode::KeyD);
    }
    assert_eq!(config(&app).seats, 5);
    assert_eq!(
        config(&app).map,
        MapChoice::GenLarge,
        "the closest that seats five"
    );
    assert_eq!(
        config(&app).pick,
        Pick::Moved(Card::Map(MapChoice::GenSmall))
    );
    let said = note(&app);
    assert!(
        said.contains(EN.setup_map_names[MapChoice::GenSmall.index()])
            && said.contains(EN.setup_map_names[MapChoice::GenLarge.index()])
            && said.contains('5'),
        "{said}"
    );
    tap(&mut app, KeyCode::KeyA);
    assert_eq!(config(&app).map, MapChoice::GenSmall, "the pick is back");
    assert_eq!(config(&app).pick, Pick::InForce);
    assert_eq!(
        moved_note(&EN, config(&app), &CustomBeaches::default()),
        None
    );

    // A large beach picked for five stays when the table shrinks to two.
    let mut large = MatchConfig {
        seats: 5,
        map: MapChoice::GenLarge,
        ..MatchConfig::default()
    };
    reseat(&mut large, 2, &CustomBeaches::default());
    assert_eq!(
        (large.map, large.pick),
        (MapChoice::GenLarge, Pick::InForce)
    );

    // A handmade beach for two: moved off at three, back at two, and the
    // note names it.
    let pair = CustomBeaches(vec![beach("Pair", "0 . 1")]);
    let mut config = MatchConfig::default();
    pick_card(&mut config, Card::Shelf(0), &pair);
    reseat(&mut config, 3, &pair);
    assert_ne!(config.map, MapChoice::Custom);
    let said = moved_note(&EN, &config, &pair).expect("moved");
    assert!(said.contains("Pair") && said.contains('3'), "{said}");
    reseat(&mut config, 2, &pair);
    assert_eq!(
        (config.map, config.custom, config.pick),
        (MapChoice::Custom, 0, Pick::InForce)
    );
}

/// Each step's keyboard line is its own, and the pad's line is its
/// face's legend: A goes on in steps 1 and 2, and on step 3 A starts the
/// match and Start joins, as the bridge and the claim read them.
/// Where the pad's Start would seat nobody it starts the match too, and
/// the legend and the button name it so.
#[test]
fn each_step_has_its_own_keys() {
    use crate::app::gamepad::Face;
    use crate::app::gamepad::scheme::legend;
    use crate::app::i18n::EN;
    let [game, map, players] = Step::ALL.map(|step| {
        legend(
            &EN,
            crate::app::gamepad::PadFamily::Xbox,
            &Face::MatchSetup {
                step,
                join: true,
                partners: false,
            },
        )
    });
    assert!(
        game.contains("A: next") && game.contains("B: back"),
        "{game}"
    );
    assert!(map.contains("A: next") && !map.contains("adjust"), "{map}");
    for part in ["X: rename", "A: start the match", "Start: join", "B: back"] {
        assert!(players.contains(part), "{players}");
    }
    let seated = legend(
        &EN,
        crate::app::gamepad::PadFamily::Xbox,
        &Face::MatchSetup {
            step: Step::Players,
            join: false,
            partners: false,
        },
    );
    assert!(
        seated.contains("Start/A: start the match") && !seated.contains("join"),
        "{seated}"
    );
    assert_eq!(
        start_button_label(&EN, LastDevice::PAD, false),
        "Start match (Start)"
    );
    assert_eq!(
        start_button_label(&EN, LastDevice::PAD, true),
        "Start match (A)"
    );
    assert_eq!(
        start_button_label(&EN, LastDevice::Keyboard, true),
        "Start match (Enter)"
    );
    assert!(EN.prompt_setup_players.contains("Tab") && EN.prompt_setup_game.contains("A/D"));
}

/// Step 3's Start match button holds its words and its key in every
/// language, in either hands.
#[test]
fn the_start_button_names_its_key_inside_it() {
    use super::ui::{BUTTON_FONT, START_BUTTON_W};
    for lang in crate::app::i18n::ALL_LANGS {
        let tr = lang.tr();
        for device in [LastDevice::Keyboard, LastDevice::PAD] {
            for join in [false, true] {
                let label = start_button_label(tr, device, join);
                let wide = display_px(&label, BUTTON_FONT);
                assert!(
                    wide + 24.0 <= START_BUTTON_W,
                    "{lang:?}: {label} is {wide:.0}px"
                );
            }
        }
    }
}

/// A headless app drawing step 2's pictures for `config` and `shelf`.
fn preview_app(config: MatchConfig, shelf: CustomBeaches) -> App {
    let mut app = App::new();
    app.add_plugins(bevy::state::app::StatesPlugin);
    app.init_state::<Screen>();
    app.insert_resource(State::new(Screen::MatchSetup));
    app.insert_resource(Assets::<Image>::default());
    app.insert_resource(crate::app::art::Art::blank());
    app.init_resource::<MapPreviews>();
    app.insert_resource(MatchMenu {
        step: Step::Map,
        seed: 9,
        ..MatchMenu::default()
    });
    app.insert_resource(config);
    app.insert_resource(shelf);
    app.add_systems(Update, (retire_previews, render_previews).chain());
    app
}

/// The preview cameras, the board sprites they draw, and those of the
/// sprites on the game's own layer.
fn preview_count(app: &mut App) -> (usize, usize, usize) {
    use bevy::camera::visibility::RenderLayers;
    let world = app.world_mut();
    let cameras = world
        .query_filtered::<Entity, With<PreviewCamera>>()
        .iter(world)
        .count();
    let parts = world
        .query_filtered::<Entity, With<preview::PreviewPart>>()
        .iter(world)
        .count();
    let unlayered = world
        .query_filtered::<Entity, (With<preview::PreviewPart>, Without<RenderLayers>)>()
        .iter(world)
        .count();
    (cameras, parts, unlayered)
}

/// Frames until no picture is drawing, at most `limit`.
fn settle_previews(app: &mut App, limit: usize) -> usize {
    for frame in 0..limit {
        app.update();
        if app.world().resource::<MapPreviews>().drawing() == 0 {
            return frame + 1;
        }
    }
    limit
}

/// Step 2's pictures: one per card bar Random and Surprise, and one for
/// each style Surprise's mosaic shows, a few started a frame,
/// drawn by a camera each on a layer of its own, the cameras and boards
/// gone after a few frames, redrawn only when the board they show changes,
/// and every picture let go of when match setup is left.
#[test]
fn previews_are_drawn_once_and_released_on_leaving() {
    let mut app = preview_app(
        MatchConfig::default(),
        CustomBeaches(vec![beach("Pair", "0 . 1")]),
    );
    app.update();
    let cards = cards(app.world().resource::<CustomBeaches>()).len();
    let (cameras, parts, unlayered) = preview_count(&mut app);
    assert_eq!(cameras, preview::PREVIEWS_PER_FRAME, "a few a frame");
    assert!(parts > 50, "the boards are laid out: {parts}");
    assert_eq!(unlayered, 0, "nothing of a preview on the game's own layer");
    settle_previews(&mut app, 400);
    // Every card but Random and Surprise, and Surprise's four samples.
    let pictures = cards - 2 + preview::SURPRISE_SAMPLES.len();
    assert_eq!(app.world().resource::<MapPreviews>().held(), pictures);
    assert_eq!(app.world().resource::<MapPreviews>().drawn(), pictures);
    assert_eq!(
        preview_count(&mut app),
        (0, 0, 0),
        "drawn, and the drawing put away"
    );
    let images: Vec<Handle<Image>> = cards_with_pictures(&app);
    assert!(
        images
            .iter()
            .all(|image| app.world().resource::<Assets<Image>>().contains(image))
    );
    // Nothing changed, nothing redrawn.
    app.update();
    assert_eq!(preview_count(&mut app).0, 0);
    // A new table: only the cards whose picture shows another board.
    app.world_mut().resource_mut::<MatchConfig>().seats = 3;
    app.update();
    assert!(preview_count(&mut app).0 > 0);
    settle_previews(&mut app, 200);
    app.add_systems(Last, release_previews);
    app.update();
    assert_eq!(app.world().resource::<MapPreviews>().held(), 0);
    assert_eq!(preview_count(&mut app), (0, 0, 0));
    for image in images {
        assert!(!app.world().resource::<Assets<Image>>().contains(&image));
    }
}

/// The seat count moving redraws the cards whose board moves with it,
/// and not a greyed card, which is drawn at a table of its own: dialling
/// five up to six redrew every card on step 2.
#[test]
fn a_greyed_card_is_not_redrawn_for_another_count() {
    let shelf = CustomBeaches(vec![beach("Pair", "0 . 1")]);
    let mut app = preview_app(
        MatchConfig {
            seats: 5,
            map: MapChoice::GenLarge,
            ..MatchConfig::default()
        },
        shelf,
    );
    settle_previews(&mut app, 200);
    let shelf = app.world().resource::<CustomBeaches>();
    let config = app.world().resource::<MatchConfig>().clone();
    let greyed: Vec<Card> = cards(shelf)
        .into_iter()
        .filter(|card| {
            *card != Card::Random && !card_fits(*card, 5, shelf) && !card_fits(*card, 6, shelf)
        })
        .collect();
    assert_eq!(
        greyed.len(),
        11,
        "classic, small, 12x9, the six handmade maps for two or four, Four Doors, and the pair"
    );
    // Each at the smallest table it seats: two, but three for Four Doors.
    for card in &greyed {
        let own = match card {
            Card::Map(map) => map.tables().map_or(2, |tables| tables[0]),
            Card::Shelf(_) | Card::Random => 2,
        };
        assert_eq!(
            card_config(&config, *card, shelf).map(|c| c.seats),
            Some(own)
        );
    }
    app.world_mut().resource_mut::<MatchConfig>().seats = 6;
    let mut redrawn = std::collections::BTreeSet::new();
    for _ in 0..200 {
        app.update();
        let previews = app.world().resource::<MapPreviews>();
        for card in cards(app.world().resource::<CustomBeaches>()) {
            if previews.is_drawing(card) {
                redrawn.insert(format!("{card:?}"));
            }
        }
        if previews.drawing() == 0 {
            break;
        }
    }
    for card in greyed {
        assert!(!redrawn.contains(&format!("{card:?}")), "{card:?} redrawn");
    }
    assert!(!redrawn.is_empty(), "the large beaches are drawn for six");
}

/// The cards on show are drawn before the ones scrolled out of view:
/// with a shelf of twenty-four beaches and the cursor on the last row,
/// the shelf's last row, the maps the Random card's mosaic borrows and
/// Surprise's samples go first.
#[test]
fn the_cards_on_show_are_drawn_first() {
    let shelf = CustomBeaches(
        (0..24)
            .map(|n| beach(&format!("Beach {n}"), "0 . 1"))
            .collect(),
    );
    let slots = map_slots(&shelf);
    let page = MapPage::of(&slots);
    let last = slots.len() - 1;
    let scroll = page.scrolled(0.0, last);
    let mut app = preview_app(MatchConfig::default(), shelf);
    {
        let mut menu = app.world_mut().resource_mut::<MatchMenu>();
        menu.slot = last;
        menu.scroll = scroll;
    }
    let mut first: Vec<Pic> = slots
        .iter()
        .enumerate()
        .filter_map(|(index, (_, slot))| match slot {
            Slot::Card(card @ (Card::Map(_) | Card::Shelf(_)))
                if page.slot_in_view(scroll, index) && *card != Card::Map(MapChoice::Surprise) =>
            {
                Some(Pic::Card(*card))
            }
            Slot::Card(_) | Slot::MakeOne => None,
        })
        .collect();
    assert_eq!(first.len(), CARD_COLUMNS, "the shelf's last row");
    first.extend(MOSAIC.map(|map| Pic::Card(Card::Map(map))));
    first.extend((0..preview::SURPRISE_SAMPLES.len()).map(Pic::Sample));
    let frames = first.len().div_ceil(preview::PREVIEWS_PER_FRAME);
    for _ in 0..frames {
        app.update();
    }
    let previews = app.world().resource::<MapPreviews>();
    for pic in &first {
        assert!(previews.image(*pic).is_some(), "{pic:?} not drawn first");
    }
    assert_eq!(previews.held(), first.len(), "and nothing else yet");
}

/// A picture is put away only once the renderer has drawn settled for a
/// few frames, so a shader still compiling does not leave a card blank;
/// and a renderer that never settles has the picture given up on and
/// drawn again, not left blank for good.
#[test]
fn a_preview_waits_for_the_renderer_and_tries_again() {
    let mut app = preview_app(MatchConfig::default(), CustomBeaches::default());
    let pipelines = PreviewPipelines::default();
    app.insert_resource(pipelines.clone());
    app.update();
    let drawing = |app: &App| app.world().resource::<MapPreviews>().drawing();
    for _ in 0..3 * preview::PREVIEW_FRAMES {
        app.update();
    }
    assert!(drawing(&app) > 0, "still compiling: the cameras stay");
    for _ in 0..preview::PREVIEW_FRAMES {
        pipelines.count(true);
    }
    let started = app.world().resource::<MapPreviews>().held();
    app.update();
    let previews = app.world().resource::<MapPreviews>();
    assert_eq!(previews.drawn(), started, "settled: drawn");

    // Never settling: given up after its patience, and started again.
    let mut app = preview_app(MatchConfig::default(), CustomBeaches::default());
    app.insert_resource(PreviewPipelines::default());
    app.update();
    let card = Card::Map(MapChoice::Classic);
    let first = app.world().resource::<MapPreviews>().image(Pic::Card(card));
    for _ in 0..preview::PREVIEW_PATIENCE {
        app.update();
    }
    let previews = app.world().resource::<MapPreviews>();
    assert_eq!(previews.drawn(), 0, "nothing claimed drawn");
    assert!(previews.is_drawing(card), "drawn again");
    assert_eq!(
        previews.image(Pic::Card(card)),
        first,
        "into the same picture"
    );
}

/// The pictures held for the built-in cards.
fn cards_with_pictures(app: &App) -> Vec<Handle<Image>> {
    let previews = app.world().resource::<MapPreviews>();
    MapChoice::ALL
        .into_iter()
        .filter(|&map| map != MapChoice::Custom)
        .filter_map(|map| previews.image(Pic::Card(Card::Map(map))))
        .collect()
}

/// Step 2's scroll bar shows the window's share of the page and how far
/// down it is: at the top as the step opens, following the cursor row by
/// row, at the bottom once it reaches the last row, and back at the top
/// on the way up. (The built-in maps alone fill more than the window, so
/// it is always there.)
#[test]
fn the_map_bar_follows_the_window() {
    use crate::app::menu_ui::scroll_bar::{self, reading};
    use bevy::ecs::system::RunSystemOnce;
    let bar_app = |shelf: CustomBeaches| {
        let mut app = setup(MatchConfig::default(), shelf, Step::Map);
        app.world_mut()
            .run_system_once(|mut commands: Commands| {
                commands.spawn(Node::default()).with_children(|band| {
                    scroll_bar::spawn(band, MapBar);
                });
            })
            .expect("the bar is built");
        app.add_systems(Update, follow_map_bar.after(match_setup_input));
        app.update();
        app
    };
    let bar = |app: &mut App| reading::<MapBar>(app.world_mut());
    let shelf = CustomBeaches(
        (0..5)
            .map(|n| beach(&format!("Beach {n}"), "0 . 1"))
            .collect(),
    );
    let page = MapPage::of(&map_slots(&shelf));
    let shown = ui::VIEW_H / page.height;
    let travel = 1.0 - shown;
    let at = |app: &App| (menu(app).scroll / (page.height - ui::VIEW_H)).min(1.0);
    let mut app = bar_app(shelf);
    assert_eq!(bar(&mut app), Some([0.0, shown, travel]), "at the top");
    for _ in 1..page.rows.len() {
        tap(&mut app, KeyCode::KeyS);
        let [above, thumb, below] = bar(&mut app).expect("a bar");
        assert!((thumb - shown).abs() < 1e-5, "{thumb}");
        assert!((above - travel * at(&app)).abs() < 1e-4, "{above}");
        assert!((below - travel * (1.0 - at(&app))).abs() < 1e-4, "{below}");
    }
    assert_eq!(page.row_of(menu(&app).slot), page.rows.len() - 1);
    assert_eq!(bar(&mut app), Some([travel, shown, 0.0]), "at the bottom");
    for _ in 1..page.rows.len() {
        tap(&mut app, KeyCode::KeyW);
    }
    assert_eq!(bar(&mut app), Some([0.0, shown, travel]), "back at the top");
}

/// Step 3 says "joined" only of a pad whose player pressed Start for the
/// seat; a pad the rule dealt the seat is "ready", as the keys are.
#[test]
fn only_a_start_pressed_says_joined() {
    let tr = crate::app::i18n::Lang::En.tr();
    let dealt = PlayedBy::Pad {
        number: 1,
        joined: false,
    };
    let pressed = PlayedBy::Pad {
        number: 1,
        joined: true,
    };
    assert_eq!(status_text(tr, dealt), tr.setup_status[0]);
    assert_eq!(status_text(tr, PlayedBy::Keys(0)), tr.setup_status[0]);
    assert_eq!(status_text(tr, pressed), tr.setup_status[1]);
}

/// Step 2's three sections, in order: the built-in maps (the handmade
/// ones, then the generated sizes), the player's beaches or, with none,
/// the card saying how to make one, then Surprise and Random. The dial
/// turns through the same order, so the lobby's map row and a series walk
/// the sections as step 2 lays them out.
#[test]
fn the_map_step_has_three_sections_in_the_dials_order() {
    let built_in: Vec<Slot> = [
        MapChoice::Classic,
        MapChoice::TheGates,
        MapChoice::TheMoat,
        MapChoice::LogJam,
        MapChoice::ThreeLanes,
        MapChoice::RoundTable,
        MapChoice::RoundTheWorld,
        MapChoice::Duel,
        MapChoice::TheFountain,
        MapChoice::FourDoors,
        MapChoice::GullRoad,
        MapChoice::BackToBack,
        MapChoice::GenSmall,
        MapChoice::GenClassic,
        MapChoice::GenLarge,
        MapChoice::GenXl,
        MapChoice::GenOcean,
    ]
    .map(|map| Slot::Card(Card::Map(map)))
    .to_vec();
    let chance = [
        Slot::Card(Card::Map(MapChoice::Surprise)),
        Slot::Card(Card::Random),
    ];
    let in_section = |shelf: &CustomBeaches, section: MapSection| -> Vec<Slot> {
        map_slots(shelf)
            .into_iter()
            .filter(|(of, _)| *of == section)
            .map(|(_, slot)| slot)
            .collect()
    };
    let empty = CustomBeaches::default();
    assert_eq!(in_section(&empty, MapSection::BuiltIn), built_in);
    assert_eq!(in_section(&empty, MapSection::Yours), vec![Slot::MakeOne]);
    assert_eq!(in_section(&empty, MapSection::Chance), chance);
    let sections: Vec<MapSection> = map_slots(&empty).into_iter().map(|(s, _)| s).collect();
    assert!(sections.is_sorted_by_key(|section| section.index()));
    assert!(!cards(&empty).is_empty() && cards(&empty).len() == map_slots(&empty).len() - 1);

    let shelf = CustomBeaches(vec![beach("Pair", "0 . 1"), beach("Trio", "0 1 2")]);
    assert_eq!(
        in_section(&shelf, MapSection::Yours),
        vec![Slot::Card(Card::Shelf(0)), Slot::Card(Card::Shelf(1))]
    );
    assert_eq!(in_section(&shelf, MapSection::BuiltIn), built_in);
    assert_eq!(in_section(&shelf, MapSection::Chance), chance);

    // The dial runs through the sections in turn.
    let dial: Vec<usize> = MapChoice::DIAL
        .iter()
        .map(|&map| MapSection::of(map).index())
        .collect();
    assert!(dial.is_sorted(), "{dial:?}");
    let mut config = MatchConfig {
        map: MapChoice::GenOcean,
        ..MatchConfig::default()
    };
    cycle_map(&mut config, Turn::Right, &shelf);
    assert_eq!((config.map, config.custom), (MapChoice::Custom, 0));
    cycle_map(&mut config, Turn::Right, &shelf);
    cycle_map(&mut config, Turn::Right, &shelf);
    assert_eq!(config.map, MapChoice::Surprise, "the shelf, then Surprise");
    cycle_map(&mut config, Turn::Right, &empty);
    assert_eq!(config.map, MapChoice::Classic, "and round to the start");
}

/// W/S/A/D walk step 2 across its sections: down off a section's last row
/// lands on the next section's first row in the same column, or on its
/// last card when that row is shorter, and up does the same the other
/// way; A/D walk the cards in order, from one section into the next.
#[test]
fn the_cursor_walks_across_the_sections() {
    let mut app = setup(MatchConfig::default(), CustomBeaches::default(), Step::Map);
    let slots = map_slots(&CustomBeaches::default());
    let at = |app: &App| slots[menu(app).slot].1;
    let map = |map| Slot::Card(Card::Map(map));
    assert_eq!(at(&app), map(MapChoice::Classic));
    tap(&mut app, KeyCode::KeyD);
    tap(&mut app, KeyCode::KeyD);
    tap(&mut app, KeyCode::KeyD);
    assert_eq!(
        at(&app),
        map(MapChoice::LogJam),
        "the first row's last card"
    );
    let down = [
        map(MapChoice::Duel),
        map(MapChoice::BackToBack),
        map(MapChoice::GenXl),
        // The built-in maps' short last row: its one card.
        map(MapChoice::GenOcean),
        // Into "Your maps", which holds the card saying how to make one.
        Slot::MakeOne,
        // Into "Random & Surprise", the same column.
        map(MapChoice::Surprise),
        // The last row: nowhere further down.
        map(MapChoice::Surprise),
    ];
    for want in down {
        tap(&mut app, KeyCode::KeyS);
        assert_eq!(at(&app), want);
    }
    tap(&mut app, KeyCode::KeyD);
    assert_eq!(at(&app), Slot::Card(Card::Random));
    tap(&mut app, KeyCode::KeyD);
    assert_eq!(at(&app), Slot::Card(Card::Random), "the last card");
    for want in [
        Slot::MakeOne,
        map(MapChoice::GenOcean),
        map(MapChoice::GenSmall),
        map(MapChoice::TheFountain),
        map(MapChoice::ThreeLanes),
        map(MapChoice::Classic),
        map(MapChoice::Classic),
    ] {
        tap(&mut app, KeyCode::KeyW);
        assert_eq!(at(&app), want);
    }

    // With beaches on the shelf: down from the generated row's third card
    // to the open ocean, the shelf's first, and the Random card under the
    // second column; up from Random to the shelf's last row, clamped.
    let shelf = || {
        CustomBeaches(
            (0..5)
                .map(|n| beach(&format!("Beach {n}"), "0 . 1"))
                .collect(),
        )
    };
    let mut app = setup(MatchConfig::default(), shelf(), Step::Map);
    let slots = map_slots(&shelf());
    let at = |app: &App| slots[menu(app).slot].1;
    let page = MapPage::of(&slots);
    let ocean = slots
        .iter()
        .position(|(_, slot)| *slot == map(MapChoice::GenOcean))
        .expect("the open ocean");
    app.world_mut().resource_mut::<MatchMenu>().slot = ocean;
    tap(&mut app, KeyCode::KeyS);
    assert_eq!(at(&app), Slot::Card(Card::Shelf(0)));
    tap(&mut app, KeyCode::KeyD);
    tap(&mut app, KeyCode::KeyS);
    assert_eq!(at(&app), Slot::Card(Card::Shelf(4)), "the short row's card");
    tap(&mut app, KeyCode::KeyS);
    assert_eq!(at(&app), map(MapChoice::Surprise));
    tap(&mut app, KeyCode::KeyD);
    tap(&mut app, KeyCode::KeyW);
    assert_eq!(at(&app), Slot::Card(Card::Shelf(4)), "clamped to the row");
    assert_eq!(page.rows.len(), 5 + 2 + 1);
}

/// The card saying how to make a map is walked onto like any other, and
/// its line says how, naming the menu's editor and the editor's levels as
/// they are named there; but it is not a map: resting on it picks
/// nothing, and Enter does not go on from it.
#[test]
fn the_make_one_card_explains_and_picks_nothing() {
    use crate::app::i18n::EN;
    let empty = CustomBeaches::default();
    let make_one = map_slots(&empty)
        .iter()
        .position(|(_, slot)| *slot == Slot::MakeOne)
        .expect("the card saying how to make a map");
    let mut app = setup(
        MatchConfig {
            map: MapChoice::GenOcean,
            ..MatchConfig::default()
        },
        CustomBeaches::default(),
        Step::Map,
    );
    app.world_mut().resource_mut::<MatchMenu>().slot = make_one - 1;
    tap(&mut app, KeyCode::KeyD);
    assert_eq!(menu(&app).slot, make_one);
    assert_eq!(config(&app).map, MapChoice::GenOcean, "nothing picked");
    assert_eq!(config(&app).pick, Pick::InForce);
    tap(&mut app, KeyCode::Enter);
    assert_eq!(menu(&app).step, Step::Map, "Enter did not go on");
    assert_eq!(config(&app).map, MapChoice::GenOcean);
    let note = slot_note(&EN, Slot::MakeOne, config(&app), &empty, false);
    assert!(
        note.contains("Level Editor") && note.contains(EN.ed_levels_title),
        "{note}"
    );
    let words = slot_words(&EN, Slot::MakeOne, 2, &empty, false);
    assert_eq!(words.pill, None, "not greyed: nothing is wrong with it");
    // Coming back to step 2, the cursor is on the pick, not on it.
    tap(&mut app, KeyCode::Escape);
    tap(&mut app, KeyCode::Enter);
    assert_eq!(
        map_slots(&empty)[menu(&app).slot].1,
        Slot::Card(Card::Map(MapChoice::GenOcean))
    );
}

/// Wherever the cursor goes, its card is whole in the window, and on a
/// section's first row so is the section's heading; the window never
/// scrolls past the page's ends, and it moves the least it can.
#[test]
fn the_window_keeps_the_cursor_in_view() {
    for shelf in [
        CustomBeaches::default(),
        CustomBeaches(vec![beach("Pair", "0 . 1"), beach("Trio", "0 1 2")]),
        CustomBeaches((0..9).map(|n| beach(&format!("B{n}"), "0 . 1")).collect()),
    ] {
        let slots = map_slots(&shelf);
        let page = MapPage::of(&slots);
        let check = |app: &App| {
            let (slot, scroll) = (menu(app).slot, menu(app).scroll);
            assert!(page.slot_in_view(scroll, slot), "{slot} at {scroll}");
            let row = &page.rows[page.row_of(slot)];
            if row.first {
                assert!(page.heading_in_view(scroll, row.section), "{slot}");
            }
            // The window opens on a heading or a row, never inside one.
            let lines: Vec<f32> = page
                .headings
                .iter()
                .copied()
                .chain(page.rows.iter().filter(|row| !row.first).map(|row| row.top))
                .collect();
            assert!(lines.contains(&scroll), "{scroll} in {lines:?}");
        };
        let mut app = setup(MatchConfig::default(), shelf, Step::Map);
        for key in [KeyCode::KeyD, KeyCode::KeyA] {
            for _ in 0..slots.len() {
                let before = menu(&app).scroll;
                tap(&mut app, key);
                check(&app);
                // The least move: a row already whole in view (with its
                // heading) does not scroll the page.
                let slot = menu(&app).slot;
                if page.scrolled(before, slot) == before {
                    assert_eq!(menu(&app).scroll, before);
                }
            }
        }
        for key in [KeyCode::KeyS, KeyCode::KeyW] {
            for _ in 0..page.rows.len() {
                tap(&mut app, key);
                check(&app);
            }
        }
        assert_eq!(menu(&app).scroll, 0.0, "back at the top");
    }
}

/// What step 2 shows is what the page's model says: the tiles and
/// headings whole in the window, and no others, the page moved up by the
/// scroll, and the line under it the cursor's card's.
#[test]
fn step_two_shows_what_the_window_holds() {
    use crate::app::i18n::EN;
    let empty = CustomBeaches::default();
    let slots = map_slots(&empty);
    let page = MapPage::of(&slots);
    let make_one = slots
        .iter()
        .position(|(_, slot)| *slot == Slot::MakeOne)
        .expect("the card saying how to make a map");
    let scroll = page.scrolled(0.0, make_one);
    assert!(scroll > 0.0, "the page scrolled to it");
    let mut app = App::new();
    app.insert_resource(MatchMenu {
        step: Step::Map,
        slot: make_one,
        scroll,
        ..MatchMenu::default()
    });
    app.insert_resource(MatchConfig::default());
    app.insert_resource(empty);
    app.insert_resource(GameSettings::default());
    app.init_resource::<LastDevice>();
    app.init_resource::<MapPreviews>();
    app.insert_resource(crate::app::art::Art::blank());
    app.add_systems(Update, (update_match_ui, paint_map_step).chain());
    app.update();
    app.update();
    let world = app.world_mut();
    let mut shown: Vec<usize> = world
        .query::<(&ui::TileUi, &Visibility)>()
        .iter(world)
        .filter(|(_, visibility)| **visibility != Visibility::Hidden)
        .map(|(tile, _)| tile.index())
        .collect();
    shown.sort_unstable();
    let want: Vec<usize> = (0..slots.len())
        .filter(|&slot| page.slot_in_view(scroll, slot))
        .collect();
    assert_eq!(shown, want);
    assert!(shown.contains(&make_one));
    let headings: Vec<MapSection> = world
        .query::<(&ui::SectionHeading, &Visibility)>()
        .iter(world)
        .filter(|(_, visibility)| **visibility != Visibility::Hidden)
        .map(|(heading, _)| heading.section())
        .collect();
    assert_eq!(headings, vec![MapSection::Yours]);
    let moved = world
        .query_filtered::<&Node, With<ui::MapPageUi>>()
        .single(world)
        .expect("the page")
        .margin
        .top;
    assert_eq!(moved, Val::Px(-scroll));
    let note = world
        .query_filtered::<&Text, With<ui::StepNote>>()
        .single(world)
        .expect("the note")
        .0
        .clone();
    assert_eq!(
        note,
        slot_note(
            &EN,
            Slot::MakeOne,
            &MatchConfig::default(),
            &CustomBeaches::default(),
            false
        )
    );
}

/// The host's map page names the table it picks for, and names it again
/// when the table shrinks under it, as it can between rounds when
/// somebody frozen at it leaves: the heading, like the greying, follows
/// the seats.
#[test]
fn the_host_map_heading_follows_the_table() {
    let mut app = App::new();
    app.insert_resource(MatchMenu {
        step: Step::Map,
        online: true,
        ..MatchMenu::default()
    });
    app.insert_resource(MatchConfig {
        seats: 4,
        ..MatchConfig::default()
    });
    app.insert_resource(CustomBeaches::default());
    app.insert_resource(GameSettings::default());
    app.init_resource::<LastDevice>();
    app.insert_resource(crate::app::art::Art::blank());
    app.add_systems(Update, update_host_map_ui);
    app.update();
    app.update();
    let heading = |app: &mut App| -> Vec<String> {
        let world = app.world_mut();
        world
            .query_filtered::<&Text, With<ui::HostMapTitle>>()
            .iter(world)
            .map(|text| text.0.clone())
            .collect()
    };
    assert_eq!(
        heading(&mut app),
        [ui::host_map_title(&crate::app::i18n::EN, 4)]
    );
    app.world_mut().resource_mut::<MatchConfig>().seats = 3;
    app.update();
    assert_eq!(
        heading(&mut app),
        [ui::host_map_title(&crate::app::i18n::EN, 3)]
    );
}
