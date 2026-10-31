//! Step 2 on screen: the map card, its sections of tiles, the pages
//! they turn through and the bar beside them.

use super::*;

pub(super) fn map_card(card: &mut ChildSpawnerCommands, paint: &Paint) {
    let slots = map_slots(&paint.beaches);
    let tr = paint.settings.tr();
    menu_ui::scroll_bar::beside(card, 0.0, MapBar, |column| {
        // The window, a heading and two rows tall, and the page inside it,
        // moved up by the scroll ([`paint_map_step`]).
        column
            .spawn(Node {
                height: Val::Px(VIEW_H),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|window| {
                window
                    .spawn((
                        MapPageUi,
                        Node {
                            flex_direction: FlexDirection::Column,
                            flex_shrink: 0.0,
                            row_gap: Val::Px(SECTION_GAP),
                            ..default()
                        },
                    ))
                    .with_children(|page| {
                        for section in MapSection::ALL {
                            map_section(page, tr, section, &slots);
                        }
                    });
            });
    });
    card.spawn((
        StepNote,
        mono("", NOTE_FONT, palette::PARCHMENT.with_alpha(0.65)),
        Node {
            margin: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(12.0), Val::Px(0.0)),
            ..default()
        },
    ));
}

/// One of step 2's sections: its heading over its cards, [`CARD_COLUMNS`]
/// to a row.
fn map_section(
    page: &mut ChildSpawnerCommands,
    tr: &crate::app::i18n::Tr,
    section: MapSection,
    slots: &[(MapSection, Slot)],
) {
    page.spawn(Node {
        flex_direction: FlexDirection::Column,
        flex_shrink: 0.0,
        ..default()
    })
    .with_children(|column| {
        column
            .spawn((
                SectionHeading(section),
                Node {
                    height: Val::Px(HEADING_H),
                    flex_shrink: 0.0,
                    align_items: AlignItems::FlexStart,
                    padding: UiRect::left(Val::Px(4.0)),
                    ..default()
                },
            ))
            .with_children(|heading| {
                heading.spawn((
                    Text::new(tr.setup_sections[section.index()]),
                    menu_ui::display_font(HEADING_FONT),
                    TextColor(palette::GOLD.with_alpha(0.75)),
                ));
            });
        column
            .spawn(Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(TILE_GAP),
                row_gap: Val::Px(TILE_GAP),
                ..default()
            })
            .with_children(|grid| {
                for (index, (of, slot)) in slots.iter().enumerate() {
                    if *of == section {
                        tile(grid, index, *slot);
                    }
                }
            });
    });
}

/// One map card: its picture, badge, name and size line.
fn tile(grid: &mut ChildSpawnerCommands, index: usize, slot: Slot) {
    grid.spawn((
        TileUi(index),
        Node {
            width: Val::Px(TILE_W),
            height: Val::Px(TILE_H),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(TILE_PAD)),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(10.0)),
            ..default()
        },
        BackgroundColor(Color::WHITE.with_alpha(0.05)),
        BorderColor::all(Color::NONE),
    ))
    .with_children(|tile| {
        tile.spawn(Node {
            width: Val::Px(THUMB_W),
            height: Val::Px(THUMB_H),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|thumb| {
            match slot {
                Slot::MakeOne => make_one_art(thumb),
                Slot::Card(Card::Random) => {
                    mosaic_art(thumb, index, TilePart::Quarter, MosaicMedal::Random);
                }
                Slot::Card(Card::Map(MapChoice::Surprise)) => {
                    mosaic_art(thumb, index, TilePart::Sample, MosaicMedal::Surprise);
                }
                Slot::Card(Card::Map(_) | Card::Shelf(_)) => {
                    thumb.spawn((
                        TileCell(index, TilePart::Thumb),
                        ImageNode::default().with_color(crate::app::materials::SAND),
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                    ));
                }
            }
            // The badge, top left over the picture.
            thumb
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(6.0),
                        top: Val::Px(6.0),
                        padding: UiRect::axes(Val::Px(7.0), Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(9.0)),
                        ..default()
                    },
                    BackgroundColor(palette::PILL_FILL),
                ))
                .with_children(|badge| {
                    badge.spawn((
                        TileCell(index, TilePart::Badge),
                        mono("", BADGE_FONT, palette::PARCHMENT.with_alpha(0.85)),
                    ));
                });
            // "Seats N, not M", over a greyed picture.
            thumb
                .spawn((
                    TileCell(index, TilePart::Pill),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        right: Val::Px(0.0),
                        top: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        display: Display::None,
                        ..default()
                    },
                ))
                .with_children(|middle| {
                    middle
                        .spawn((
                            Node {
                                padding: UiRect::axes(Val::Px(12.0), Val::Px(4.0)),
                                border_radius: BorderRadius::all(Val::Px(12.0)),
                                ..default()
                            },
                            BackgroundColor(palette::PILL_FILL),
                        ))
                        .with_children(|pill| {
                            pill.spawn((
                                TileCell(index, TilePart::PillText),
                                Text::new(""),
                                menu_ui::display_font(PILL_FONT),
                                TextColor(palette::PARCHMENT.with_alpha(0.85)),
                            ));
                        });
                });
        });
        // Each line in a cell that clips it, as step 1's values are: a
        // text node does not clip its own words, and a long beach name ran
        // on into the next card.
        tile.spawn(clip_cell(UiRect::new(
            Val::Px(2.0),
            Val::Px(0.0),
            Val::Px(NAME_GAP),
            Val::Px(0.0),
        )))
        .with_children(|cell| {
            cell.spawn((
                TileCell(index, TilePart::Name),
                mono("", CARD_NAME_FONT, palette::PARCHMENT),
            ));
        });
        tile.spawn(clip_cell(UiRect::left(Val::Px(2.0))))
            .with_children(|cell| {
                cell.spawn((
                    TileCell(index, TilePart::Sub),
                    mono("", CARD_SUB_FONT, palette::PARCHMENT.with_alpha(0.6)),
                ));
            });
    });
}

/// A cell as wide as a card's picture, past `margin`, that clips the line
/// inside it.
fn clip_cell(margin: UiRect) -> Node {
    Node {
        width: Val::Px(THUMB_W - NAME_INSET),
        margin,
        overflow: Overflow::clip_x(),
        ..default()
    }
}

/// How far a card's lines sit in from its picture's left edge.
pub(in crate::app::match_setup) const NAME_INSET: f32 = 2.0;

/// A handmade beach's name as its card says it: whole when it fits the
/// card's picture, else cut short with "…" where it would run past it.
/// The one name a card, its note and step 3's line give a beach.
pub(in crate::app::match_setup) fn card_name(name: &str) -> String {
    cut_to(name, THUMB_W - NAME_INSET, CARD_NAME_FONT)
}

/// The picture of the card saying how to make a map: bare sand under a
/// driftwood medal with a plus on it, a beach still to be built.
fn make_one_art(thumb: &mut ChildSpawnerCommands) {
    thumb
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(crate::app::materials::SAND.darker(0.35)),
        ))
        .with_children(|middle| {
            medal(
                middle,
                palette::MEDAL_DRIFTWOOD,
                palette::MEDAL_DIGIT,
                "+",
                72.0,
            );
        });
}

/// The medal over a mosaic card's picture.
#[derive(Clone, Copy)]
enum MosaicMedal {
    /// Random's gold question mark: one of the maps on show.
    Random,
    /// Surprise's silver exclamation mark: a beach nobody has seen yet.
    Surprise,
}

/// A mosaic card's own picture: four pictures, darkened, under a medal.
/// Random borrows four of the other cards' pictures ([`MOSAIC`]), Surprise
/// shows a beach in each of four of its styles ([`SURPRISE_SAMPLES`]).
fn mosaic_art(
    thumb: &mut ChildSpawnerCommands,
    index: usize,
    part: fn(usize) -> TilePart,
    medal_of: MosaicMedal,
) {
    thumb
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            ..default()
        })
        .with_children(|mosaic| {
            for quarter in 0..4 {
                mosaic.spawn((
                    TileCell(index, part(quarter)),
                    ImageNode::default().with_color(crate::app::materials::SAND.darker(0.3)),
                    Node {
                        width: Val::Percent(50.0),
                        height: Val::Percent(50.0),
                        ..default()
                    },
                ));
            }
        });
    thumb
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|middle| {
            middle
                .spawn((
                    Node {
                        padding: UiRect::all(Val::Px(4.0)),
                        border: UiRect::all(Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(50.0)),
                        ..default()
                    },
                    BorderColor::all(match medal_of {
                        MosaicMedal::Random => palette::GOLD.with_alpha(0.55),
                        MosaicMedal::Surprise => palette::MEDAL_SILVER.with_alpha(0.55),
                    }),
                ))
                .with_children(|ring| {
                    let (fill, mark) = match medal_of {
                        MosaicMedal::Random => (palette::MEDAL_GOLD, "?"),
                        MosaicMedal::Surprise => (palette::MEDAL_SILVER, "!"),
                    };
                    medal(ring, fill, palette::MEDAL_DIGIT, mark, 84.0)
                });
        });
}

/// What step 2's tiles are painted from.
type TilePaint = (
    &'static TileCell,
    Option<&'static mut Text>,
    Option<&'static mut TextColor>,
    Option<&'static mut ImageNode>,
    Option<&'static mut Node>,
);

/// Step 2's tiles, and nothing else.
type Tiles = (
    &'static TileUi,
    &'static mut BackgroundColor,
    &'static mut BorderColor,
    &'static mut Visibility,
);

/// Step 2's headings and page, and nothing else with a node.
type PageParts = (
    Option<&'static SectionHeading>,
    Option<&'static mut Visibility>,
    &'static mut Node,
);

/// Step 2's headings and the page they sit on, and no tile or its parts.
type PageOnly = (
    Or<(With<SectionHeading>, With<MapPageUi>)>,
    Without<TileUi>,
    Without<TileCell>,
);

/// Paint step 2: the page scrolled to the cursor, with only the rows and
/// headings whole in the window shown, and each tile's picture, words,
/// pick and greying, and the note.
pub fn paint_map_step(
    paint: Paint,
    previews: Res<MapPreviews>,
    mut tiles: Query<Tiles, Without<TileCell>>,
    mut page_parts: Query<PageParts, PageOnly>,
    mut cells: Query<TilePaint, (Without<TileUi>, Without<StepNote>)>,
    mut note: Query<&mut Text, (With<StepNote>, Without<TileCell>)>,
) {
    if paint.menu.step != Step::Map {
        return;
    }
    let tr = paint.settings.tr();
    let config = &*paint.config;
    let slots = map_slots(&paint.beaches);
    let page = MapPage::of(&slots);
    let at = paint.menu.slot.min(slots.len() - 1);
    let scroll = paint.menu.scroll;
    let picked = Slot::Card(picked_card(config, &paint.beaches));
    let online = paint.menu.online;
    let fits = |slot: Slot| match slot {
        Slot::Card(card) => card_open(card, config.seats, &paint.beaches, online),
        Slot::MakeOne => false,
    };
    for (heading, visibility, mut node) in &mut page_parts {
        match (heading, visibility) {
            (Some(heading), Some(mut visibility)) => {
                set_visible(&mut visibility, page.heading_in_view(scroll, heading.0));
            }
            (Some(_), None) | (None, _) => {
                let top = Val::Px(-scroll);
                if node.margin.top != top {
                    node.margin.top = top;
                }
            }
        }
    }
    for (tile, mut fill, mut edge, mut visibility) in &mut tiles {
        let slot = slots[tile.0].1;
        let is_picked = slot == picked;
        set_visible(&mut visibility, page.slot_in_view(scroll, tile.0));
        menu_ui::set_bg(
            &mut fill,
            match is_picked {
                true => palette::PICKED_WASH,
                false => Color::WHITE.with_alpha(0.05),
            },
        );
        // The pick wears the cards' gold hairline at full strength; the
        // cursor resting on a card it cannot pick, a parchment one.
        let want = match (is_picked, tile.0 == at && !fits(slot)) {
            (true, _) => palette::GOLD,
            (false, true) => palette::PARCHMENT.with_alpha(0.6),
            (false, false) => Color::NONE,
        };
        if *edge != BorderColor::all(want) {
            *edge = BorderColor::all(want);
        }
    }
    // Worked out once a card, when one of its cells first asks: a card is
    // a dozen cells, and the words are formatted strings.
    let mut said: Vec<Option<TileWords>> = slots.iter().map(|_| None).collect();
    for (cell, text, ink, image, node) in &mut cells {
        let slot = slots[cell.0].1;
        // The card saying how to make a map reads as one to walk onto, not
        // greyed: there is nothing wrong with it, only nothing to pick.
        let lit = fits(slot) || slot == Slot::MakeOne;
        let words =
            match cell.1 {
                TilePart::Thumb | TilePart::Quarter(_) | TilePart::Sample(_) => None,
                TilePart::Pill
                | TilePart::PillText
                | TilePart::Badge
                | TilePart::Name
                | TilePart::Sub => Some(&*said[cell.0].get_or_insert_with(|| {
                    slot_words(tr, slot, config.seats, &paint.beaches, online)
                })),
            };
        let is_picked = slot == picked;
        match cell.1 {
            TilePart::Thumb => {
                let Slot::Card(card) = slot else {
                    continue;
                };
                if let (Some(mut image), Some(drawn)) = (image, previews.image(Pic::Card(card)))
                    && image.image != drawn
                {
                    image.image = drawn;
                    image.color = Color::WHITE;
                }
            }
            TilePart::Quarter(quarter) | TilePart::Sample(quarter) => {
                let pic = match matches!(cell.1, TilePart::Sample(_)) {
                    true => Pic::Sample(quarter),
                    false => Pic::Card(Card::Map(MOSAIC[quarter])),
                };
                if let (Some(mut image), Some(drawn)) = (image, previews.image(pic))
                    && image.image != drawn
                {
                    image.image = drawn;
                    // Darkened, so the medal reads over it.
                    image.color = Color::srgb(0.5, 0.48, 0.45);
                }
            }
            TilePart::Pill => {
                if let (Some(mut node), Some(words)) = (node, words) {
                    menu_ui::set_shown(&mut node, words.pill.is_some());
                }
            }
            TilePart::PillText | TilePart::Badge | TilePart::Name | TilePart::Sub => {
                let (Some(mut text), Some(mut ink), Some(words)) = (text, ink, words) else {
                    continue;
                };
                let (line, color) = match cell.1 {
                    TilePart::PillText => (
                        words.pill.clone().unwrap_or_default(),
                        palette::PARCHMENT.with_alpha(0.85),
                    ),
                    TilePart::Badge => (
                        words.badge.to_string(),
                        match slot {
                            Slot::Card(Card::Shelf(_)) | Slot::MakeOne => palette::GOLD,
                            Slot::Card(Card::Map(_) | Card::Random) => {
                                palette::PARCHMENT.with_alpha(0.85)
                            }
                        },
                    ),
                    TilePart::Name => (
                        words.name.clone(),
                        match (lit, is_picked) {
                            (false, _) => palette::PARCHMENT.with_alpha(0.30),
                            (true, true) => Color::WHITE,
                            (true, false) => palette::PARCHMENT.with_alpha(0.85),
                        },
                    ),
                    TilePart::Sub => (
                        words.sub.clone(),
                        palette::PARCHMENT.with_alpha(if lit { 0.6 } else { 0.25 }),
                    ),
                    TilePart::Thumb
                    | TilePart::Pill
                    | TilePart::Quarter(_)
                    | TilePart::Sample(_) => continue,
                };
                menu_ui::set_text(&mut text, &line);
                menu_ui::set_color(&mut ink, color);
            }
        }
    }
    if let Ok(mut text) = note.single_mut() {
        let line = slot_note(tr, slots[at].1, config, &paint.beaches, online);
        menu_ui::set_text(&mut text, &line);
    }
}

/// Show or hide a part of step 2's page, keeping its place in the layout:
/// the page is laid out whole and moved, and a part hidden must not move
/// the rest.
fn set_visible(visibility: &mut Mut<Visibility>, shown: bool) {
    let want = match shown {
        true => Visibility::Inherited,
        false => Visibility::Hidden,
    };
    if **visibility != want {
        **visibility = want;
    }
}

/// Keep step 2's scroll bar on the window's place down the page.
pub fn follow_map_bar(
    menu: Res<MatchMenu>,
    beaches: Res<CustomBeaches>,
    mut bars: menu_ui::scroll_bar::ScrollBars<MapBar>,
) {
    if menu.step == Step::Map {
        let page = MapPage::of(&map_slots(&beaches));
        bars.show(page.window(menu.scroll));
    }
}
