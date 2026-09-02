//! What the lobby puts on the screen: two faces, and the strip beneath
//! them.
//!
//! Built once on entry; the systems here only ever write text and colour
//! into what is already there, never spawn. Which face shows depends on
//! whether you are choosing a beach or standing on one. The line being
//! typed belongs to neither, because a name is asked for before there is
//! any beach to be at.

use super::*;

/// How many beaches the join list shows at once. A busy LAN can have more
/// games running than this; the list scrolls to the cursor rather than
/// pretending the rest are not there.
pub const LIST_ROWS: usize = 14;

#[derive(Component)]
pub struct LobbyUi;

#[derive(Component)]
pub struct LobbyRow(pub usize);

/// The four things a row of the beach list says, left to right.
///
/// The middle two are the ones an address alone never answered: a hall of
/// eight beaches all called something sensible still leaves "which one is
/// Bo's?" and "which of these is the machine by the window?" unanswered,
/// and the answers used to live only in the beacon.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListCol {
    /// What the beach is called, behind its number.
    Name,
    /// The player who put it up.
    Host,
    /// Where it is, written the way somebody would have to type it.
    Where,
    /// How full it is, and whether the round has begun.
    Table,
}

/// One column of one row. A single component with a column in it rather
/// than four marker components: the painter reads them all off one query,
/// where four `Text` queries in one system would each need three `Without`
/// filters to prove to Bevy that they cannot alias.
#[derive(Component)]
pub struct LobbyCell(pub usize, pub ListCol);

/// Lines of chat kept and shown. Still not a place to scroll back
/// through, but the panel is the wide half of the screen now and a
/// six-line strip in it looked like an afterthought.
pub const CHAT_LINES: usize = 14;

/// The name half of a feed line, and on row [`CHAT_LINES`] the line being
/// typed.
#[derive(Component)]
pub struct ChatRow(pub usize);

/// The other half: what was actually said. A span of its own because the
/// two halves are painted apart (the name in its seat's colour, a notice
/// in the slanted face) and a `Text` carries one ink and one font.
#[derive(Component)]
pub struct ChatSaid(pub usize);

/// The box the line is typed into. Drawn only when there is a line to
/// type: an empty highlighted strip under an empty feed reads as something
/// broken rather than as something waiting.
#[derive(Component)]
pub struct ChatEntryBox;

/// The two faces of the lobby. Browsing is a hall full of games and wants
/// the whole width for them; once you are at a beach the games are somebody
/// else's business, and the space belongs to who is here and what they are
/// saying.
#[derive(Component)]
pub struct BrowseView;

#[derive(Component)]
pub struct TableView;

#[derive(Component)]
pub struct PlayerRowName(pub usize);

#[derive(Component)]
pub struct DialRow(pub usize);

#[derive(Component)]
pub struct DialName(pub usize);

#[derive(Component)]
pub struct DialValue(pub usize);

/// The line under the terms saying why the map dial offers none of the
/// host's own beaches. A lobby fills its seats as people arrive, so a table
/// can outgrow the beach the host picked while the host is watching.
#[derive(Component)]
pub struct DialBeachNote;

/// Which of the lobby's four cards is being built. Only the decoration
/// depends on it, but the decoration is the point: a card of empty rows
/// with nothing on it is a hole in the screen.
#[derive(Clone, Copy)]
pub enum LobbyCard {
    Beaches,
    Table,
    Round,
    Chat,
}

/// The sprites the lobby wears.
///
/// Copied handles rather than a borrowed [`crate::app::art::Art`], and a
/// `Default` of empty ones, so the lobby can still be built in a test app
/// with no asset server behind it, which is where the prompt-row test
/// builds it.
#[derive(Default)]
pub struct LobbyArt {
    boat: Handle<Image>,
    castle: Handle<Image>,
    star: Handle<Image>,
    crab: Handle<Image>,
    foam: Handle<Image>,
}

impl LobbyArt {
    pub fn from_art(art: &crate::app::art::Art) -> Self {
        Self {
            boat: art.boat.clone(),
            castle: art.castle.clone(),
            star: art.star.clone(),
            crab: art.crab.clone(),
            foam: art.foam.clone(),
        }
    }

    /// What a card wears beside its heading: a boat for the beaches you
    /// could sail to, a castle for the one you are standing on, a sparkle
    /// for the terms, a crab for the talk.
    fn icon(&self, card: LobbyCard) -> &Handle<Image> {
        match card {
            LobbyCard::Beaches => &self.boat,
            LobbyCard::Table => &self.castle,
            LobbyCard::Round => &self.star,
            LobbyCard::Chat => &self.crab,
        }
    }
}

/// The lobby's furniture: a card of beaches to join, and a card of things
/// said. Built once on entry; the systems below only ever write text and
/// colour into it, never spawn.
pub(super) fn spawn_lobby_ui(commands: &mut Commands, tr: &crate::app::i18n::Tr, art: &LobbyArt) {
    let full = || Node {
        position_type: PositionType::Absolute,
        // Started just under the header rather than a screen's-worth below
        // it: the left column carries six player rows and seven dial rows,
        // which at 720 (the default height) needs every pixel between the
        // header and the foot, or the last dials spill out of their card.
        top: Val::Px(60.0),
        left: Val::Px(28.0),
        right: Val::Px(28.0),
        // Clear of the chat-entry line and the prompt below it. The prompt
        // (`hud::PromptLabel`, 22px) wraps to two rows when it is long - the
        // host's is - reaching roughly 70px up from the foot, and the entry
        // line sits above that; the panels stop above the entry line so the
        // three never stack into each other.
        bottom: Val::Px(116.0),
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(14.0),
        ..default()
    };
    spawn_browse_face(commands, &full, tr, art);
    spawn_table_face(commands, &full, tr, art);
    spawn_entry_bar(commands);
}

/// Show the face that applies: the hall of beaches, or the one you are at.
pub fn update_lobby_view(
    state: Res<LobbyState>,
    mut browse: Query<&mut Node, (With<BrowseView>, Without<TableView>)>,
    mut table: Query<&mut Node, (With<TableView>, Without<BrowseView>)>,
) {
    let aboard = state.standing().at_a_beach();
    for mut node in &mut browse {
        crate::app::menu_ui::set_shown(&mut node, !aboard);
    }
    for mut node in &mut table {
        crate::app::menu_ui::set_shown(&mut node, aboard);
    }
}

/// The ink a seat's name reads in, wherever its name is written: on the
/// table, and on everything that seat says in the feed beside it.
fn seat_tone(seat: usize) -> Color {
    crate::app::palette::player_color(seat as u8).lighter(0.15)
}

/// The ink a name in the feed reads in. A name with no seat at the table,
/// a watcher or somebody who has since walked off, reads idle rather than
/// borrowing a colour that belongs to a player who is still here.
fn name_ink(table: &[String], who: &str) -> Color {
    match table.iter().position(|seat| seat == who) {
        Some(seat) => seat_tone(seat),
        None => palette::IDLE_ROW,
    }
}

/// Which line of the feed lands on a given row. The feed packs to the
/// bottom, so the newest is always on the last row and the empties are
/// above it: a chat that grew downward from the top would drift away from
/// the box it is typed into as it filled.
fn line_at(chat: &[Said], row: usize) -> Option<&Said> {
    // How far back from the newest, which sits on the last row.
    let back = (CHAT_LINES - 1).checked_sub(row)?;
    chat.len().checked_sub(back + 1).map(|at| &chat[at])
}

/// Paint the table: everyone at this beach, the local player first.
pub fn update_lobby_players(
    state: Res<LobbyState>,
    mut rows: Query<(&PlayerRowName, &mut Text, &mut TextColor)>,
) {
    for (row, mut text, mut color) in &mut rows {
        let (line, tone) = match state.table.get(row.0) {
            Some(who) => (format!("{}. {who}", row.0 + 1), seat_tone(row.0)),
            None => (String::new(), Color::NONE),
        };
        crate::app::menu_ui::set_text(&mut text, &line);
        crate::app::menu_ui::set_color(&mut color, tone);
    }
}

/// One panel in the shared card language: a hairline edge over a dark
/// fill, on the same soft shadow every other screen's cards stand on, with
/// its heading above the rows. A `width` of `Auto` takes whatever the
/// other panel leaves.
fn card(
    screen: &mut RelatedSpawnerCommands<ChildOf>,
    art: &LobbyArt,
    which: LobbyCard,
    heading: &str,
    width: Val,
    rows: impl FnOnce(&mut RelatedSpawnerCommands<ChildOf>),
) {
    screen
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                width,
                flex_grow: if width == Val::Auto { 1.0 } else { 0.0 },
                row_gap: Val::Px(4.0),
                // Room at the foot for the tide to come in without
                // washing over the last row.
                padding: UiRect::all(Val::Px(12.0))
                    .with_bottom(Val::Px(crate::app::menu_ui::FOAM_DEPTH + 6.0)),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BorderColor::all(palette::CARD_EDGE),
            BackgroundColor(palette::CARD_FILL),
            crate::app::menu_ui::card_shadow(),
        ))
        .with_children(|panel| {
            crate::app::menu_ui::tide_line(panel, &art.foam);
            crate::app::menu_ui::heading_row(panel, heading, Some(art.icon(which)));
            rows(panel);
        });
}

fn row_font(px: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(px),
        ..default()
    }
}

fn row_text(px: f32) -> (Text, TextFont, TextColor) {
    (Text::new(""), row_font(px), TextColor(palette::PARCHMENT))
}

/// A row that must stay one line tall whatever it is given. Only the terms
/// need it: a wrapped dial makes its row two lines, its card taller, and
/// every card under it move. Chat is left to wrap, because a message cut
/// off at the panel edge is worse than a feed that reflows.
fn fixed_row_text(px: f32) -> (Text, TextFont, TextColor, TextLayout) {
    let (text, font, color) = row_text(px);
    (text, font, color, TextLayout::no_wrap())
}

/// One column of the beach list: a fixed width so the four line up down the
/// whole list, and clipped rather than wrapped.
///
/// Both halves of that matter. A ragged middle column is unreadable at a
/// dozen rows, since the eye scans a column and not a row. And a name too
/// long for its width would otherwise wrap and make its row twice as tall
/// as its neighbours, which is worse than losing a letter off the end. Names are
/// capped at [`crate::transport::WIRE_NAME`] on the wire and the widths
/// below are cut to hold that many characters of this font, so in practice
/// nothing is clipped at all; the clip is for a window narrower than the
/// list wants, where every column gives up the same fraction.
fn list_cell(px: f32, width: Val, grow: f32) -> impl Bundle {
    (
        Text::new(""),
        row_font(px),
        TextColor(palette::PARCHMENT),
        TextLayout::no_wrap(),
        Node {
            width,
            flex_grow: grow,
            overflow: Overflow::clip_x(),
            ..default()
        },
    )
}

/// How wide the columns are, each cut to hold the widest thing that can go
/// in it: a whole [`crate::transport::WIRE_NAME`] of mono at that size, an
/// address as long as `255.255.255.255:65535`, a table as long as
/// `6/6  (en cours)`, and for the first column a name with a two-figure
/// row number in front of it.
///
/// Which name, whose beach and where it is are the three that answer "is
/// this the game I am looking for", and they sit in a block together; the
/// slack of a wide window goes into the gap before the last column, which
/// answers the different question of whether there is a way in. The name
/// column is fixed like the rest rather than taking the slack itself,
/// which left the host's name stranded an inch to the right of the beach
/// it belongs to.
const NAME_COL: f32 = 380.0;
const HOST_COL: f32 = 250.0;
const WHERE_COL: f32 = 215.0;
const TABLE_COL: f32 = 190.0;

/// A row's two inks: the one the beach's own name reads in, and the one
/// the smaller print beside it does.
///
/// Under the cursor both come up; a beach with no way in goes down in
/// both, so a full one reads as unavailable at a glance rather than after
/// reading its count. In between, the name carries the row and the details
/// sit behind it. Four columns all in parchment is a wall of text.
fn row_ink(picked: bool, room: bool) -> (Color, Color) {
    match (picked, room) {
        (true, _) => (palette::SELECTED_ROW, palette::PARCHMENT),
        (false, true) => (palette::PARCHMENT, palette::IDLE_ROW),
        (false, false) => (palette::IDLE_ROW.darker(0.1), palette::IDLE_ROW.darker(0.2)),
    }
}

/// Paint the join list: one row per beach on the air, scrolled to the
/// cursor. Hosting empties it, since a host cannot join anyone and its
/// own listening socket is handed back the moment it starts.
pub fn update_lobby_list(
    state: Res<LobbyState>,
    settings: Res<GameSettings>,
    mut rows: Query<(&LobbyRow, &mut BackgroundColor)>,
    mut cells: Query<(&LobbyCell, &mut Text, &mut TextColor)>,
) {
    let tr = settings.tr();
    let listing = !state.standing().at_a_beach();
    let at = state.selected_index();
    let host_at = |row: usize| -> Option<&HostEntry> {
        listing
            .then(|| state.hosts.get(state.scroll + row))
            .flatten()
    };
    for (row, mut fill) in &mut rows {
        let picked = host_at(row.0).is_some() && Some(state.scroll + row.0) == at;
        let want = crate::app::menu_ui::band(picked);
        crate::app::menu_ui::set_bg(&mut fill, want);
    }
    for (cell, mut text, mut color) in &mut cells {
        let LobbyCell(row, col) = *cell;
        let (line, tone) = match host_at(row) {
            Some(host) => {
                let (name_ink, side_ink) = row_ink(Some(state.scroll + row) == at, host.has_room());
                match col {
                    ListCol::Name => (
                        format!("{}. {}", state.scroll + row + 1, host.who()),
                        name_ink,
                    ),
                    ListCol::Host => (host.creator().to_string(), side_ink),
                    ListCol::Where => (host.addr.to_string(), side_ink),
                    ListCol::Table => (host.table(tr), host.table_tone()),
                }
            }
            None => (String::new(), Color::NONE),
        };
        crate::app::menu_ui::set_text(&mut text, &line);
        crate::app::menu_ui::set_color(&mut color, tone);
    }
}

/// Say why the map dial is offering none of the host's own beaches, under
/// the terms it belongs to. A system of its own rather than another pair of
/// queries on [`update_lobby_terms`]: that one already reaches for `Text`
/// twice, and a third would have to spell out what it is not.
pub fn update_lobby_beach_note(
    settings: Res<GameSettings>,
    config: Res<MatchConfig>,
    beaches: Res<crate::app::match_setup::CustomBeaches>,
    mut note: Query<&mut Text, With<DialBeachNote>>,
) {
    let Ok(mut text) = note.single_mut() else {
        return;
    };
    let line =
        crate::app::match_setup::beaches_note(&config, settings.tr(), &beaches).unwrap_or_default();
    crate::app::menu_ui::set_text(&mut text, &line);
}

/// Paint the terms. The host's cursor shows on them; a joiner reads them.
pub fn update_lobby_terms(
    state: Res<LobbyState>,
    settings: Res<GameSettings>,
    config: Res<MatchConfig>,
    beaches: Res<crate::app::match_setup::CustomBeaches>,
    mut rows: Query<(&DialRow, &mut BackgroundColor)>,
    mut names: Query<(&DialName, &mut Text, &mut TextColor), Without<DialValue>>,
    mut values: Query<(&DialValue, &mut Text, &mut TextColor), Without<DialName>>,
) {
    let tr = settings.tr();
    let host = state.hosting();
    // A joiner reads the host's dials, off the roster, not its own setup
    // screen's: the card is meant to show the match being joined. The host,
    // and a chooser not yet at any beach, read their own config.
    let joined = (!host)
        .then(|| {
            state
                .joined()
                .and_then(|joined| joined.terms)
                .map(|t| crate::app::match_setup::config_from_terms(&t))
        })
        .flatten();
    let (config, team_mode): (&MatchConfig, _) = match &joined {
        Some((cfg, mode)) => (cfg, *mode),
        None => (&config, settings.team_mode),
    };
    for (row, mut fill) in &mut rows {
        let want = crate::app::menu_ui::band(host && row.0 == state.dial);
        crate::app::menu_ui::set_bg(&mut fill, want);
    }
    for (row, mut text, mut color) in &mut names {
        let line = Dial::ALL
            .get(row.0)
            .map(|dial| dial.label(tr, config, team_mode, &beaches).0)
            .unwrap_or_default();
        crate::app::menu_ui::set_text(&mut text, line);
        crate::app::menu_ui::set_color(&mut color, palette::IDLE_ROW);
    }
    for (row, mut text, mut color) in &mut values {
        let line = Dial::ALL
            .get(row.0)
            .map(|dial| dial.label(tr, config, team_mode, &beaches).1)
            .unwrap_or_default();
        // Only the host can turn them, so only the host's read as live.
        let tone = match host {
            true => palette::PARCHMENT,
            false => palette::IDLE_ROW,
        };
        crate::app::menu_ui::set_text(&mut text, &line);
        crate::app::menu_ui::set_color(&mut color, tone);
    }
}

/// Swap a line between the upright face and the slanted one, guarded like
/// every other write here: a `TextFont` touched at all re-shapes the line
/// it belongs to, and most frames nothing in the feed has changed.
fn set_slant(font: &mut Mut<TextFont>, slanted: bool) {
    let want = match slanted {
        true => FontSource::Handle(crate::app::boot::ITALIC_FONT),
        false => FontSource::default(),
    };
    if font.font != want {
        font.font = want;
    }
}

/// Paint the chat feed, oldest line at the top, with whatever is being
/// typed on the row beneath it.
///
/// Each line is two pieces: the name, in the colour its seat wears on the
/// table, and what was said. A notice, somebody arriving or leaving, has
/// no name and takes the slanted face instead, so the lobby's own remarks
/// do not read as one more player talking.
pub fn update_lobby_chat(
    state: Res<LobbyState>,
    settings: Res<GameSettings>,
    mut rows: Query<(&ChatRow, &mut Text, &mut TextColor), Without<ChatSaid>>,
    mut said: Query<(&ChatSaid, &mut TextSpan, &mut TextColor, &mut TextFont), Without<ChatRow>>,
    mut entry: Query<&mut BackgroundColor, With<ChatEntryBox>>,
) {
    let tr = settings.tr();
    // No beach joined and nothing being typed means there is nothing to
    // type into, and a lit box with nothing in it looks like a fault.
    let box_lit = state.typing.is_some() || state.can_chat();
    for mut fill in &mut entry {
        let want = crate::app::menu_ui::band(box_lit);
        crate::app::menu_ui::set_bg(&mut fill, want);
    }
    for (row, mut text, mut color) in &mut rows {
        let prompt_row = row.0 == CHAT_LINES;
        let (line, tone) = match (prompt_row, state.typing.as_ref()) {
            (true, Some(open)) => {
                let asked = match open.what {
                    Entry::PlayerName => tr.lobby_ask_player_name,
                    Entry::GameName => tr.lobby_ask_game_name,
                    Entry::Address => tr.lobby_ask_address,
                    Entry::Chat => "",
                };
                (format!("{asked}{}_", open.text), palette::GOLD)
            }
            (true, None) if state.can_chat() => (tr.lobby_chat_hint.to_string(), palette::IDLE_ROW),
            (true, None) => (String::new(), Color::NONE),
            // Only the name lives here; the words are on the span beside
            // it. A notice was said by nobody, and leaves this half empty.
            (false, _) => match line_at(&state.chat, row.0) {
                Some(said) if !said.is_notice() => {
                    (format!("{}: ", said.who), name_ink(&state.table, &said.who))
                }
                _ => (String::new(), Color::NONE),
            },
        };
        crate::app::menu_ui::set_text(&mut text, &line);
        crate::app::menu_ui::set_color(&mut color, tone);
    }
    for (row, mut span, mut color, mut font) in &mut said {
        let (line, tone, slanted) = match line_at(&state.chat, row.0) {
            Some(said) => (said.line.as_str(), palette::PARCHMENT, said.is_notice()),
            None => ("", Color::NONE, false),
        };
        crate::app::menu_ui::set_text(&mut span, line);
        crate::app::menu_ui::set_color(&mut color, tone);
        set_slant(&mut font, slanted);
    }
}

/// The face a player reads while choosing: every beach on the air,
/// across the whole width, because that is all there is to do here.
fn spawn_browse_face(
    commands: &mut Commands,
    full: &dyn Fn() -> Node,
    tr: &crate::app::i18n::Tr,
    art: &LobbyArt,
) {
    commands
        .spawn((LobbyUi, BrowseView, full()))
        .with_children(|screen| {
            card(
                screen,
                art,
                LobbyCard::Beaches,
                tr.lobby_card_beaches,
                Val::Auto,
                |body| {
                    for row in 0..LIST_ROWS {
                        body.spawn((
                            LobbyRow(row),
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(16.0),
                                padding: UiRect::axes(Val::Px(12.0), Val::Px(4.0)),
                                border_radius: BorderRadius::all(Val::Px(6.0)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|line| {
                            line.spawn((
                                LobbyCell(row, ListCol::Name),
                                list_cell(21.0, Val::Px(NAME_COL), 0.0),
                            ));
                            line.spawn((
                                LobbyCell(row, ListCol::Host),
                                list_cell(17.0, Val::Px(HOST_COL), 0.0),
                            ));
                            // The address takes the slack of a wide window,
                            // which puts the gap between the three columns
                            // that say which beach and the one that says
                            // whether it can be joined.
                            line.spawn((
                                LobbyCell(row, ListCol::Where),
                                list_cell(17.0, Val::Px(WHERE_COL), 1.0),
                            ));
                            line.spawn((
                                LobbyCell(row, ListCol::Table),
                                list_cell(19.0, Val::Px(TABLE_COL), 0.0),
                            ));
                        });
                    }
                },
            );
        });
}

/// The face a player reads once they are somewhere: who is here, what
/// the round is set to, and what everyone is saying.
fn spawn_table_face(
    commands: &mut Commands,
    full: &dyn Fn() -> Node,
    tr: &crate::app::i18n::Tr,
    art: &LobbyArt,
) {
    // At a beach: who is here, and what is being said.
    commands
        .spawn((LobbyUi, TableView, full()))
        .with_children(|screen| {
            screen
                .spawn(Node {
                    // Wide enough for the longest map name in German, which
                    // is what sets it: the terms are the only rows on this
                    // screen with a value that long, and the chat beside
                    // them had more room than it was using.
                    width: Val::Percent(42.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(14.0),
                    ..default()
                })
                .with_children(|column| {
                    card(
                        column,
                        art,
                        LobbyCard::Table,
                        tr.lobby_card_players,
                        Val::Auto,
                        |body| {
                            for row in 0..crate::sim::MAX_PLAYERS {
                                body.spawn(Node {
                                    padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                                    ..default()
                                })
                                .with_children(|line| {
                                    line.spawn((PlayerRowName(row), row_text(21.0)));
                                });
                            }
                        },
                    );
                    // The terms. Everyone sees them, since a joiner is
                    // entitled to know what it is joining, and only the
                    // host turns them.
                    card(
                        column,
                        art,
                        LobbyCard::Round,
                        tr.lobby_card_terms,
                        Val::Auto,
                        |body| {
                            for row in 0..Dial::ALL.len() {
                                body.spawn((
                                    DialRow(row),
                                    Node {
                                        justify_content: JustifyContent::SpaceBetween,
                                        align_items: AlignItems::Center,
                                        padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                                        border_radius: BorderRadius::all(Val::Px(6.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                ))
                                .with_children(|line| {
                                    // A gutter for the name and the rest for
                                    // the value, both told not to wrap. Left
                                    // to themselves the longest map name
                                    // wraps in German, which makes the row
                                    // two lines tall, the card taller, and
                                    // the whole column jump every time the
                                    // host turns the dial. It also lands the
                                    // value on top of the name.
                                    line.spawn((
                                        Node {
                                            flex_shrink: 0.0,
                                            ..default()
                                        },
                                        children![(DialName(row), fixed_row_text(18.0))],
                                    ));
                                    line.spawn((
                                        Node {
                                            flex_grow: 1.0,
                                            flex_basis: Val::Px(0.0),
                                            min_width: Val::Px(0.0),
                                            justify_content: JustifyContent::End,
                                            overflow: Overflow::clip_x(),
                                            ..default()
                                        },
                                        children![(DialValue(row), fixed_row_text(18.0))],
                                    ));
                                });
                            }
                            // Under the dials, in a row that keeps its
                            // height empty or not, so the card does not
                            // grow a line as the table fills up.
                            //
                            // The row text's own ink is dropped for a
                            // quieter one - passed as a fourth component it
                            // would be a second `TextColor` on the entity,
                            // which Bevy refuses outright.
                            let (text, font, _, layout) = fixed_row_text(14.0);
                            body.spawn((
                                Node {
                                    height: Val::Px(20.0),
                                    padding: UiRect::horizontal(Val::Px(10.0)),
                                    overflow: Overflow::clip_x(),
                                    ..default()
                                },
                                children![(
                                    DialBeachNote,
                                    text,
                                    font,
                                    layout,
                                    TextColor(palette::PARCHMENT.with_alpha(0.55)),
                                )],
                            ));
                        },
                    );
                });
            card(
                screen,
                art,
                LobbyCard::Chat,
                tr.lobby_card_chat,
                Val::Auto,
                |body| {
                    // The feed takes the slack and packs to the bottom, so what
                    // was just said sits next to where the next thing is typed
                    // rather than drifting away from it up the panel.
                    body.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        justify_content: JustifyContent::FlexEnd,
                        row_gap: Val::Px(3.0),
                        ..default()
                    })
                    .with_children(|feed| {
                        for row in 0..CHAT_LINES {
                            // Name and words are one line on screen and two
                            // pieces underneath: a span carries its own ink
                            // and its own face, a `Text` carries one of each.
                            feed.spawn((ChatRow(row), row_text(19.0)))
                                .with_children(|line| {
                                    line.spawn((
                                        ChatSaid(row),
                                        TextSpan::new(""),
                                        row_font(19.0),
                                        TextColor(palette::PARCHMENT),
                                    ));
                                });
                        }
                    });
                },
            );
        });
}

/// The line being typed, which belongs to neither face: a name is asked
/// for while browsing, before there is a beach to be at.
///
/// It used to live inside the chat card, on the face that is hidden then,
/// so pressing H opened a question nobody could see and turned every other
/// key into text. It sits under both faces now.
fn spawn_entry_bar(commands: &mut Commands) {
    commands
        .spawn((
            LobbyUi,
            ChatEntryBox,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(28.0),
                right: Val::Px(28.0),
                // Above the prompt's two-row band (see `spawn_lobby_ui`):
                // at 52 it landed under the host's wrapped prompt and the
                // two read as one smear of overlapping text.
                bottom: Val::Px(76.0),
                padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|entry| {
            entry.spawn((ChatRow(CHAT_LINES), row_text(20.0)));
        });
}

#[cfg(test)]
mod list_row_tests {
    use super::*;

    /// A beach on the air, as the list holds it.
    fn beach(last: u8, name: &str, host: &str, taken: u8, running: bool) -> HostEntry {
        HostEntry {
            addr: format!("10.0.0.{last}:47777").parse().expect("addr"),
            id: u64::from(last),
            name: name.to_string(),
            host: host.to_string(),
            taken,
            seats: 6,
            running,
            age: 0.0,
        }
    }

    /// What every row of the list actually reads, column by column, painted
    /// by the system that paints it on screen.
    fn painted(hosts: Vec<HostEntry>, cursor: Option<SocketAddr>) -> Vec<[(String, Color); 4]> {
        let mut app = App::new();
        app.insert_resource(GameSettings::default());
        app.init_resource::<LobbyState>();
        {
            let mut state = app.world_mut().resource_mut::<LobbyState>();
            state.hosts = hosts;
            state.selected = cursor;
        }
        app.add_systems(
            Startup,
            |mut commands: Commands, settings: Res<GameSettings>| {
                spawn_lobby_ui(&mut commands, settings.tr(), &LobbyArt::default());
            },
        );
        app.add_systems(Update, update_lobby_list);
        app.update();

        let mut rows = vec![[const { (String::new(), Color::NONE) }; 4]; LIST_ROWS];
        let world = app.world_mut();
        let mut cells = world.query::<(&LobbyCell, &Text, &TextColor)>();
        for (cell, text, color) in cells.iter(world) {
            let LobbyCell(row, col) = *cell;
            let at = match col {
                ListCol::Name => 0,
                ListCol::Host => 1,
                ListCol::Where => 2,
                ListCol::Table => 3,
            };
            rows[row][at] = (text.0.clone(), color.0);
        }
        rows
    }

    /// The whole of a row, as somebody scanning the hall reads it: which
    /// game, whose it is, where it is, and whether there is a way in.
    ///
    /// The middle two are the ones an address in the corner of a screen
    /// never answered. "Room 3" does not say it is Anna's, and "Anna's
    /// beach" does not say which of the eight machines in the room it is
    /// running on, which is the thing a friend on a LAN with no broadcast
    /// has to type in by hand.
    #[test]
    fn a_row_says_which_game_whose_and_where() {
        let hosts = vec![
            beach(1, "Room 3", "Anna", 2, false),
            beach(2, "The Pier", "Bo", 6, true),
        ];
        let there = hosts[0].addr;
        let rows = painted(hosts, Some(there));

        let [name, who, at, table] = &rows[0];
        assert_eq!(name.0, "1. Room 3", "the beach, behind the number shouted");
        assert_eq!(who.0, "Anna", "whose it is, which its name never says");
        assert_eq!(at.0, "10.0.0.1:47777", "and where, to the letter");
        assert_eq!(table.0, "2/6");
        assert_eq!(
            at.0.parse::<SocketAddr>().ok(),
            Some(there),
            "what is written is what Enter dials: read out to a friend \
             across the room, it has to reach this beach and no other"
        );

        // The second row is the same shape, and says the round has begun.
        let [name, who, at, table] = &rows[1];
        assert_eq!((name.0.as_str(), who.0.as_str()), ("2. The Pier", "Bo"));
        assert_eq!(at.0, "10.0.0.2:47777");
        assert!(table.0.contains(crate::app::i18n::EN.lobby_full_tag));

        // And every row past the beaches is blank in all four columns
        // rather than holding the last thing that stood there.
        for row in &rows[2..] {
            assert!(
                row.iter().all(|(line, _)| line.is_empty()),
                "a row with no beach on it says nothing: {row:?}"
            );
        }
    }

    /// The row under the cursor comes up, a beach with no way in goes down,
    /// and the small print follows its row rather than holding its own
    /// colour. Otherwise a full beach still reads as an invitation in
    /// three columns out of four.
    #[test]
    fn the_details_take_the_colour_of_the_row_they_are_on() {
        let hosts = vec![
            beach(1, "Room 3", "Anna", 2, false),
            beach(2, "Full", "Bo", 6, false),
        ];
        let rows = painted(hosts, Some("10.0.0.1:47777".parse().expect("addr")));

        let (lit_name, lit_side) = row_ink(true, true);
        assert_eq!(rows[0][0].1, lit_name, "the cursor's row is the lit one");
        assert_eq!(rows[0][1].1, lit_side);
        assert_eq!(rows[0][2].1, lit_side, "the address with it");

        let (dim_name, dim_side) = row_ink(false, false);
        assert_eq!(rows[1][0].1, dim_name, "a full beach is not an offer");
        assert_eq!(rows[1][2].1, dim_side, "and neither is its address");
        assert_ne!(lit_side, dim_side, "the two say different things");
    }
}

#[cfg(test)]
mod feed_layout_tests {
    use super::*;

    /// Which row each line lands on, through the same [`line_at`] the
    /// painter uses.
    fn rows(said: &[&str]) -> Vec<String> {
        let mut state = LobbyState::default();
        for line in said {
            state.say("", line);
        }
        (0..CHAT_LINES)
            .map(|row| match line_at(&state.chat, row) {
                Some(said) => said.line.clone(),
                None => String::new(),
            })
            .collect()
    }

    #[test]
    fn the_newest_line_is_always_on_the_last_row() {
        let empty = rows(&[]);
        assert!(
            empty.iter().all(String::is_empty),
            "nothing said, nothing shown"
        );

        let one = rows(&["ready?"]);
        assert_eq!(one.last().unwrap(), "ready?", "on the bottom, not the top");
        assert!(one[..CHAT_LINES - 1].iter().all(String::is_empty));

        let two = rows(&["ready?", "wait for me"]);
        assert_eq!(two[CHAT_LINES - 2], "ready?");
        assert_eq!(two[CHAT_LINES - 1], "wait for me");

        // Full, and then one more: the oldest falls off the top.
        let many: Vec<String> = (0..CHAT_LINES + 1).map(|n| format!("line {n}")).collect();
        let full = rows(&many.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(full[0], "line 1", "line 0 has gone");
        assert_eq!(full[CHAT_LINES - 1], format!("line {CHAT_LINES}"));
        assert!(
            full.iter().all(|line| !line.is_empty()),
            "no gaps once full"
        );

        // The prompt is a `ChatRow` too, one past the feed, and no line of
        // the feed belongs to it.
        assert!(line_at(&[], CHAT_LINES).is_none());
    }

    /// A name in the feed wears its seat's colour, the same one the table
    /// above the feed writes it in. That is why it is coloured at all, so
    /// the two are read off one function.
    #[test]
    fn a_name_reads_in_its_seats_colour() {
        let table = vec!["Anna".to_string(), "Bo".to_string()];
        assert_eq!(name_ink(&table, "Anna"), seat_tone(0));
        assert_eq!(name_ink(&table, "Bo"), seat_tone(1));
        assert_ne!(seat_tone(0), seat_tone(1), "one colour per seat");
        // A watcher, or somebody who left: no seat, so no seat's colour.
        assert_eq!(name_ink(&table, "Cy"), palette::IDLE_ROW);
        assert_eq!(name_ink(&[], "Anna"), palette::IDLE_ROW);
    }
}
