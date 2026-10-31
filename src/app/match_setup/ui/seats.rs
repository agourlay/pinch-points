//! Step 3 on screen: who plays which seat, the teams, and the start
//! button.

use super::*;

/// Step 3's card: the column heads, a row for every seat a table can have
/// (those past the table folded), team by team under a team mode with
/// each team's heading over it, the lines under them and the button.
pub(super) fn seats_card(
    card: &mut ChildSpawnerCommands,
    tr: &crate::app::i18n::Tr,
    paint: &Paint,
) {
    let art = &*paint.art;
    let mode = setup_teams(&paint.settings, &paint.config);
    // The column heads.
    card.spawn(Node {
        flex_direction: FlexDirection::Row,
        padding: UiRect::horizontal(Val::Px(10.0)),
        margin: UiRect::bottom(Val::Px(2.0)),
        ..default()
    })
    .with_children(|heads| {
        for (index, width) in [CHIP_W + 10.0, NAME_W + 8.0, BY_W].into_iter().enumerate() {
            heads.spawn((
                Text::new(tr.setup_columns[index]),
                menu_ui::display_font(13.0),
                TextColor(palette::GOLD.with_alpha(0.55)),
                Node {
                    width: Val::Px(width),
                    ..default()
                },
            ));
        }
    });
    let heads: Vec<(u8, Option<u8>)> = match mode {
        crate::app::teams::TeamMode::Solo => {
            (0..MAX_PLAYERS as u8).map(|seat| (seat, None)).collect()
        }
        crate::app::teams::TeamMode::Pairs | crate::app::teams::TeamMode::Trios => {
            crate::app::lineup::seat_order(mode, paint.config.seats)
        }
    };
    for (seat, heads) in heads {
        if let Some(team) = heads {
            card.spawn((
                TeamHeadingUi(team),
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(TEAM_HEADING_FONT),
                    ..default()
                },
                TextLayout::no_wrap(),
                TextColor(palette::PARCHMENT),
                Node {
                    margin: UiRect::new(
                        Val::Px(12.0),
                        Val::Px(10.0),
                        Val::Px(if team > 0 { 4.0 } else { 0.0 }),
                        Val::Px(0.0),
                    ),
                    overflow: Overflow::clip_x(),
                    ..default()
                },
            ));
        }
        card.spawn((
            SeatRowUi(seat),
            Node {
                height: Val::Px(SEAT_PITCH - 4.0),
                margin: UiRect::vertical(Val::Px(2.0)),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(10.0)),
                border_radius: BorderRadius::all(Val::Px(7.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|row| seat_row(row, seat, tr, art));
    }
    card.spawn(hairline(0.18));
    card.spawn((
        SeatMoved,
        mono("", SEAT_HINT_FONT, palette::GOLD),
        Node {
            margin: UiRect::horizontal(Val::Px(10.0)),
            display: Display::None,
            ..default()
        },
    ));
    card.spawn((
        SeatHint,
        mono("", SEAT_HINT_FONT, palette::PARCHMENT.with_alpha(0.55)),
        Node {
            margin: UiRect::horizontal(Val::Px(10.0)),
            ..default()
        },
    ));
    card.spawn((
        PartnerLine,
        mono("", SEAT_HINT_FONT, palette::PARCHMENT.with_alpha(0.55)),
        Node {
            margin: UiRect::horizontal(Val::Px(10.0)),
            display: Display::None,
            ..default()
        },
    ));
    card.spawn((
        SeatsLine,
        mono("", ROW_FONT - 2.0, palette::GOLD),
        Node {
            align_self: AlignSelf::Center,
            margin: UiRect::top(Val::Px(8.0)),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
            border_radius: BorderRadius::all(Val::Px(9.0)),
            min_height: Val::Px(25.0),
            ..default()
        },
        BackgroundColor(Color::NONE),
    ));
    button(
        card,
        String::new(),
        START_BUTTON_W,
        AlignSelf::Center,
        StartButtonText,
    );
}

/// One seat's row: its chip, name, who plays it and its status.
fn seat_row(
    row: &mut ChildSpawnerCommands,
    seat: u8,
    tr: &crate::app::i18n::Tr,
    art: &crate::app::art::Art,
) {
    // The chip, after the side panels' score chips: the seat's colour as
    // the fill, its paint as the edge, a medal disc with its number.
    row.spawn((
        Node {
            width: Val::Px(CHIP_W),
            height: Val::Px(34.0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            padding: UiRect::left(Val::Px(6.0)),
            margin: UiRect::right(Val::Px(10.0)),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(9.0)),
            ..default()
        },
        BackgroundColor(chip_fill(seat)),
        BorderColor::all(palette::seat_paint(seat)),
    ))
    .with_children(|chip| {
        let disc = palette::leader_chip(seat);
        medal(
            chip,
            disc,
            palette::ink_on(disc),
            &(seat + 1).to_string(),
            22.0,
        );
        chip.spawn((
            Text::new(crate::app::seat_label(tr, seat)),
            menu_ui::display_font(19.0),
            TextColor(palette::name_ink_on(chip_fill(seat))),
        ));
    });
    row.spawn((
        SeatCell(seat, SeatPart::Name),
        menu_ui::cell(NAME_W, ROW_FONT),
    ));
    row.spawn(Node {
        width: Val::Px(8.0),
        flex_shrink: 0.0,
        ..default()
    });
    // The icon: the keys, a pad, or the robot the bot tag wears.
    row.spawn(Node {
        width: Val::Px(ICON_W),
        height: Val::Px(26.0),
        flex_shrink: 0.0,
        margin: UiRect::right(Val::Px(10.0)),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    })
    .with_children(|icon| {
        keys_icon(icon, seat);
        pad_icon(icon, seat);
        icon.spawn((
            SeatCell(seat, SeatPart::RobotIcon),
            ImageNode::new(art.robot.clone()).with_color(palette::GOLD.with_alpha(0.85)),
            Node {
                width: Val::Px(26.0),
                height: Val::Px(26.0),
                display: Display::None,
                ..default()
            },
        ));
    });
    row.spawn((
        SeatCell(seat, SeatPart::By),
        mono("", ROW_FONT, palette::PARCHMENT),
        Node {
            width: Val::Px(BY_W - ICON_W - 10.0),
            flex_shrink: 0.0,
            overflow: Overflow::clip_x(),
            ..default()
        },
    ));
    row.spawn((
        SeatCell(seat, SeatPart::Status),
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(STATUS_FONT),
            ..default()
        },
        TextColor(palette::INK_ONE_POST),
        Node {
            width: Val::Px(STATUS_W),
            flex_grow: 1.0,
            justify_content: JustifyContent::FlexEnd,
            ..default()
        },
        TextLayout::new(Justify::Right, LineBreak::NoWrap),
    ));
}

/// A seat chip's fill: the seat's colour, darkened for the white label on
/// it, and as far again as the label needs ([`palette::legible`]). Scaled
/// channel by channel, so every seat keeps its hue.
fn chip_fill(seat: u8) -> Color {
    let color = palette::player_color(seat).to_srgba();
    palette::legible(Color::srgb(
        color.red * 0.6,
        color.green * 0.6,
        color.blue * 0.6,
    ))
}

/// A keyboard drawn in the interface's own lines: a rounded case and two
/// rows of keys over a space bar.
fn keys_icon(icon: &mut ChildSpawnerCommands, seat: u8) {
    icon.spawn((
        SeatCell(seat, SeatPart::KeysIcon),
        Node {
            width: Val::Px(28.0),
            height: Val::Px(18.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: Val::Px(1.5),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            display: Display::None,
            ..default()
        },
        BorderColor::all(palette::GOLD.with_alpha(0.85)),
    ))
    .with_children(|case| {
        for row in 0..2 {
            case.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(1.5),
                ..default()
            })
            .with_children(|keys| {
                for _ in 0..(5 - row) {
                    keys.spawn((
                        Node {
                            width: Val::Px(2.5),
                            height: Val::Px(2.5),
                            ..default()
                        },
                        BackgroundColor(palette::GOLD.with_alpha(0.85)),
                    ));
                }
            });
        }
        case.spawn((
            Node {
                width: Val::Px(12.0),
                height: Val::Px(2.0),
                ..default()
            },
            BackgroundColor(palette::GOLD.with_alpha(0.85)),
        ));
    });
}

/// A controller in the same lines: a rounded body, a d-pad's cross and
/// four face buttons.
fn pad_icon(icon: &mut ChildSpawnerCommands, seat: u8) {
    let dot = |size: f32| {
        (
            Node {
                width: Val::Px(size),
                height: Val::Px(size),
                border_radius: BorderRadius::all(Val::Px(size / 2.0)),
                ..default()
            },
            BackgroundColor(palette::GOLD.with_alpha(0.85)),
        )
    };
    icon.spawn((
        SeatCell(seat, SeatPart::PadIcon),
        Node {
            width: Val::Px(30.0),
            height: Val::Px(18.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::horizontal(Val::Px(3.0)),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(8.0)),
            display: Display::None,
            ..default()
        },
        BorderColor::all(palette::GOLD.with_alpha(0.85)),
    ))
    .with_children(|body| {
        // The cross: a bar and a post through it.
        body.spawn(Node {
            width: Val::Px(7.0),
            height: Val::Px(7.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|cross| {
            cross.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(7.0),
                    height: Val::Px(2.0),
                    ..default()
                },
                BackgroundColor(palette::GOLD.with_alpha(0.85)),
            ));
            cross.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(2.0),
                    height: Val::Px(7.0),
                    ..default()
                },
                BackgroundColor(palette::GOLD.with_alpha(0.85)),
            ));
        });
        body.spawn(Node {
            width: Val::Px(8.0),
            height: Val::Px(8.0),
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            justify_content: JustifyContent::SpaceBetween,
            align_content: AlignContent::SpaceBetween,
            ..default()
        })
        .with_children(|buttons| {
            for _ in 0..4 {
                buttons.spawn(dot(3.0));
            }
        });
    });
}

/// What step 3 is painted from beyond the model: the pads seated.
#[derive(bevy::ecs::system::SystemParam)]
pub struct SeatsPaint<'w, 's> {
    pads: Res<'w, crate::app::gamepad::PadSeats>,
    live: Query<'w, 's, Entity, With<Gamepad>>,
    /// The pad last reached with, whose maker's name for Start the lines
    /// about a pad's Start say.
    last_pad: Res<'w, crate::app::gamepad::LastPad>,
    gamepads: Query<'w, 's, &'static Gamepad>,
}

/// What step 3's rows are painted with.
type SeatPaint = (
    &'static SeatCell,
    Option<&'static mut Text>,
    Option<&'static mut TextColor>,
    Option<&'static mut TextFont>,
    Option<&'static mut Node>,
    Option<&'static mut BorderColor>,
);

/// Step 3's hint, and nothing else that has text.
type HintLine = (With<SeatHint>, Without<SeatCell>, Without<SeatsLine>);

/// Step 3's moved-map line, and nothing else with text or a node.
type MovedLine = (
    With<SeatMoved>,
    Without<SeatHint>,
    Without<SeatCell>,
    Without<SeatsLine>,
    Without<SeatRowUi>,
);

/// The line under step 3's seats, and nothing else with a fill.
type SeatsLineOnly = (
    With<SeatsLine>,
    Without<SeatRowUi>,
    Without<SeatHint>,
    Without<SeatCell>,
);

/// Paint step 3: each seat's name, who plays it and its status, the rows
/// past the table folded, the cursor's band, and the lines under them.
pub fn paint_players_step(
    paint: Paint,
    seated: SeatsPaint,
    mut rows: Query<(&SeatRowUi, &mut BackgroundColor, &mut Node), Without<SeatCell>>,
    mut cells: Query<SeatPaint, (Without<SeatRowUi>, Without<SeatsLine>)>,
    mut hint: Query<(&mut Text, &mut TextColor), HintLine>,
    mut moved: Query<(&mut Text, &mut Node), MovedLine>,
    mut line: Query<(&mut Text, &mut BackgroundColor), SeatsLineOnly>,
) {
    let menu = &*paint.menu;
    if menu.step != Step::Players {
        return;
    }
    let tr = paint.settings.tr();
    let config = &*paint.config;
    let live: Vec<Entity> = seated.live.iter().collect();
    let family =
        crate::app::gamepad::reference_family(*paint.device, *seated.last_pad, &seated.gamepads);
    let pads = seated
        .pads
        .seat_pads(&live, &paint.settings, &config.human_seats());
    let at = menu.seat.min(config.seats - 1);
    for (row, mut fill, mut node) in &mut rows {
        menu_ui::set_shown(&mut node, row.0 < config.seats);
        menu_ui::set_bg(&mut fill, menu_ui::band(row.0 == at));
    }
    for (cell, text, ink, font, node, edge) in &mut cells {
        let seat = cell.0;
        if seat >= config.seats {
            continue;
        }
        let by = played_by(
            config,
            seat,
            &pads,
            seated.pads.p2_at_keys(),
            &paint.settings,
        );
        let picked = seat == at;
        match cell.1 {
            SeatPart::KeysIcon | SeatPart::PadIcon | SeatPart::RobotIcon => {
                let icon = match by {
                    PlayedBy::Keys(_) => SeatPart::KeysIcon,
                    PlayedBy::Pad { .. } | PlayedBy::Open { .. } => SeatPart::PadIcon,
                    PlayedBy::Ai(_) | PlayedBy::Bot => SeatPart::RobotIcon,
                };
                let shown = cell.1 == icon;
                if let Some(mut node) = node {
                    menu_ui::set_shown(&mut node, shown);
                }
                // A pad waiting for a friend is drawn faint.
                if let Some(mut edge) = edge {
                    let want = BorderColor::all(match by {
                        PlayedBy::Open { .. } => palette::PARCHMENT.with_alpha(0.35),
                        PlayedBy::Keys(_)
                        | PlayedBy::Pad { .. }
                        | PlayedBy::Ai(_)
                        | PlayedBy::Bot => palette::GOLD.with_alpha(0.85),
                    });
                    if *edge != want {
                        *edge = want;
                    }
                }
            }
            SeatPart::Name | SeatPart::By | SeatPart::Status => {
                let (Some(mut text), Some(mut ink)) = (text, ink) else {
                    continue;
                };
                let (words, color, px) = match cell.1 {
                    SeatPart::Name => seat_name_cell(tr, &paint, seat, by, picked),
                    SeatPart::By => {
                        let words = played_by_text(tr, by, family);
                        match (by, picked && seat > 0) {
                            (PlayedBy::Open { .. }, _) => (words, palette::GOLD, OPEN_FONT),
                            // The dial's `< >` under the cursor, on every
                            // seat A/D turns.
                            (
                                PlayedBy::Keys(_)
                                | PlayedBy::Pad { .. }
                                | PlayedBy::Ai(_)
                                | PlayedBy::Bot,
                                true,
                            ) => (format!("< {words} >"), Color::WHITE, ROW_FONT),
                            (
                                PlayedBy::Keys(_)
                                | PlayedBy::Pad { .. }
                                | PlayedBy::Ai(_)
                                | PlayedBy::Bot,
                                false,
                            ) => (words, row_ink(picked), ROW_FONT),
                        }
                    }
                    SeatPart::Status
                    | SeatPart::KeysIcon
                    | SeatPart::PadIcon
                    | SeatPart::RobotIcon => (
                        status_text(tr, by).to_string(),
                        match by {
                            PlayedBy::Keys(_) | PlayedBy::Pad { .. } => {
                                palette::INK_ONE_POST.with_alpha(0.95)
                            }
                            PlayedBy::Open { .. } | PlayedBy::Ai(_) | PlayedBy::Bot => {
                                palette::PARCHMENT.with_alpha(0.45)
                            }
                        },
                        STATUS_FONT,
                    ),
                };
                menu_ui::set_text(&mut text, &words);
                menu_ui::set_color(&mut ink, color);
                if let Some(mut font) = font
                    && font.font_size != FontSize::Px(px)
                {
                    font.font_size = FontSize::Px(px);
                }
            }
        }
    }
    // A pad's Start that added a seat can move the map after step 2
    // showed it: the match is played on the one named here.
    if let Ok((mut text, mut node)) = moved.single_mut() {
        let note = moved_note(tr, config, &paint.beaches);
        menu_ui::set_text(&mut text, note.as_deref().unwrap_or(""));
        menu_ui::set_shown(&mut node, note.is_some());
    }
    if let Ok((mut text, mut ink)) = hint.single_mut() {
        menu_ui::set_text(&mut text, &seat_hint(tr, *paint.device, at));
        menu_ui::set_color(&mut ink, palette::PARCHMENT.with_alpha(0.55));
    }
    if let Ok((mut text, mut pill)) = line.single_mut() {
        let open = open_seats(config, &pads, seated.pads.p2_at_keys(), &paint.settings);
        let pad_to_join =
            seated
                .pads
                .a_start_would_seat_someone(&live, &paint.settings, &config.human_seats());
        let (words, warns) = seats_line(tr, &open, config.ai_level, (pad_to_join, family));
        menu_ui::set_text(&mut text, &words);
        menu_ui::set_bg(
            &mut pill,
            match warns {
                true => palette::PILL_FILL,
                false => Color::NONE,
            },
        );
    }
}

/// A seat's name cell: the name, the box being typed into, or for a seat
/// somebody plays with no name yet, the key that names it.
fn seat_name_cell(
    tr: &crate::app::i18n::Tr,
    paint: &Paint,
    seat: u8,
    by: PlayedBy,
    picked: bool,
) -> (String, Color, f32) {
    let given = &paint.settings.names[usize::from(paint.config.name_row(seat))];
    if paint.menu.naming == Some(seat) {
        // A caret says the keyboard is being typed into rather than
        // navigating; the row is its own instructions.
        let keep = crate::app::gamepad::scheme::spelled(
            tr,
            *paint.device,
            &crate::app::gamepad::Face::Typing(crate::app::gamepad::scheme::TextBox::Keeps),
            crate::app::gamepad::scheme::Does::Confirm,
            tr.key_enter,
        );
        return (
            format!("{given}_ {}", fill(tr.match_name_typing, &[("k", &keep)])),
            Color::WHITE,
            STATUS_FONT,
        );
    }
    if !given.is_empty() {
        return (given.clone(), row_ink(picked), ROW_FONT);
    }
    match by {
        PlayedBy::Keys(_) | PlayedBy::Pad { .. } => (
            unnamed(tr, *paint.device),
            palette::PARCHMENT.with_alpha(0.45),
            STATUS_FONT,
        ),
        PlayedBy::Open { .. } | PlayedBy::Ai(_) | PlayedBy::Bot => (
            "-".to_string(),
            palette::PARCHMENT.with_alpha(0.35),
            ROW_FONT,
        ),
    }
}

/// Step 3's Start match button, naming the key that starts the match in
/// the hands last used: Enter at the keyboard, and on a pad the first
/// button the step's own table starts it with
/// ([`crate::app::gamepad::scheme::binds`]), Start once that pad's Start
/// seats nobody new, else A.
pub fn start_button_label(tr: &crate::app::i18n::Tr, device: LastDevice, join: bool) -> String {
    use crate::app::gamepad::scheme::{Does, spelled};
    let face = crate::app::gamepad::Face::MatchSetup {
        step: Step::Players,
        join,
        partners: false,
    };
    let key = spelled(tr, device, &face, Does::StartMatch, tr.key_enter);
    fill(tr.setup_start, &[("k", &key)])
}

/// Paint step 3's Start match button's words ([`start_button_label`]).
pub fn paint_start_button(
    paint: Paint,
    seated: SeatsPaint,
    last_pad: Res<crate::app::gamepad::LastPad>,
    mut words: Query<&mut Text, With<StartButtonText>>,
) {
    if paint.menu.step != Step::Players {
        return;
    }
    let live: Vec<Entity> = seated.live.iter().collect();
    let join = seated.pads.start_joins_here(
        &live,
        &paint.settings,
        &paint.config.human_seats(),
        last_pad.0,
    );
    let label = start_button_label(paint.settings.tr(), *paint.device, join);
    for mut text in &mut words {
        menu_ui::set_text(&mut text, &label);
    }
}

/// Step 3's line about teams, and no team heading.
type PartnerLineOnly = (With<PartnerLine>, Without<TeamHeadingUi>);

/// Paint step 3's team headings, each its seats' names joined in the
/// colour of the seat standing for it, as the line-up heads a team
/// ([`crate::app::lineup::team_heading`]), and the line about teams under
/// the hint ([`partner_line`]).
pub fn paint_teams(
    paint: Paint,
    seated: SeatsPaint,
    mut headings: Query<(&TeamHeadingUi, &mut Text, &mut TextColor), Without<PartnerLine>>,
    mut line: Query<(&mut Text, &mut Node), PartnerLineOnly>,
) {
    if paint.menu.step != Step::Players {
        return;
    }
    let tr = paint.settings.tr();
    let config = &*paint.config;
    let mode = setup_teams(&paint.settings, config);
    let names = crate::app::SeatNames(config.seat_names(&paint.settings.names));
    for (heading, mut text, mut ink) in &mut headings {
        let (words, color) =
            crate::app::lineup::team_heading(tr, &names, mode, heading.0, config.seats);
        menu_ui::set_text(&mut text, &words);
        menu_ui::set_color(&mut ink, color);
    }
    let at = paint.menu.seat.min(config.seats - 1);
    let live: Vec<Entity> = seated.live.iter().collect();
    let pads = seated
        .pads
        .seat_pads(&live, &paint.settings, &config.human_seats());
    let pin = p2_pin(config, &paint.settings, &pads, seated.pads.p2_at_keys());
    let said = partner_line(tr, *paint.device, (config, &paint.settings), at, pin);
    for (mut text, mut node) in &mut line {
        menu_ui::set_text(&mut text, said.as_deref().unwrap_or(""));
        menu_ui::set_shown(&mut node, said.is_some());
    }
}
