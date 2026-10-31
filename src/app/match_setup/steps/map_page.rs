//! Step 2's page: the grid it lays the cards out on, the host's own map
//! page, and their keys.

use super::*;

/// The lobby host's map page while it is up ([`MatchMenu::online`]),
/// whether opened at the start of a table or between its rounds: what to
/// put back, and whether it opened this frame.
pub struct HostPage {
    /// The match setup as it was before the page opened: put back whole
    /// on Esc, and its seat count put back after a pick, the couch's table
    /// being nothing to do with this one.
    before: MatchConfig,
    /// The page opened this frame, on the Enter that put it up: that
    /// Enter is not also a pick.
    fresh: bool,
}

/// How the host's map page closes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Close {
    /// A card was picked: kept, on the couch's own seat count.
    Keep,
    /// Esc, or the table changing under it: the setup as it was.
    PutBack,
}

impl HostPage {
    /// Put the page up on `config` as the caller dealt it for the table,
    /// `before` the setup as it stood: the cursor on the pick, and a fresh
    /// seed for the pictures, which the match is then played on, so the
    /// beach previewed is the beach played.
    pub fn open(
        before: MatchConfig,
        config: &MatchConfig,
        menu: &mut MatchMenu,
        beaches: &CustomBeaches,
    ) -> Self {
        menu.online = true;
        menu.step = Step::Map;
        menu.seed = crate::app::clock::fresh_seed();
        menu.scroll = 0.0;
        put_the_cursor_on_the_pick(menu, config, beaches);
        HostPage {
            before,
            fresh: true,
        }
    }

    /// A page up since an earlier frame, over the setup `before`.
    #[cfg(test)]
    pub fn settled(before: MatchConfig) -> Self {
        HostPage {
            before,
            fresh: false,
        }
    }

    /// Whether the page opened this frame, answered once: the Enter that
    /// opened it is let go.
    pub fn just_opened(&mut self) -> bool {
        std::mem::take(&mut self.fresh)
    }

    /// Take the page down.
    pub fn close(
        self,
        close: Close,
        config: &mut MatchConfig,
        menu: &mut MatchMenu,
        beaches: &CustomBeaches,
    ) {
        match close {
            Close::PutBack => *config = self.before,
            // The pick stays the host's (the next page opens on it, and so
            // does the couch's step 2), on the couch's own table.
            Close::Keep => reseat(config, self.before.seats, beaches),
        }
        menu.online = false;
        menu.step = Step::Game;
    }
}

impl MatchMenu {
    /// Whether the host's map page is up: what the pause card, the face
    /// and the prompt stand down for.
    pub fn host_page_up(&self) -> bool {
        self.online && self.step == Step::Map
    }
}

/// Step 2's grid is this many cards across, in every section.
pub const CARD_COLUMNS: usize = 4;

/// The cards the Random card's mosaic shows: four of the built-in maps,
/// whose pictures it borrows.
pub const MOSAIC: [MapChoice; 4] = [
    MapChoice::Classic,
    MapChoice::GenLarge,
    MapChoice::GenOcean,
    MapChoice::GenXl,
];

/// One row of step 2's page: the places on it ([`map_slots`] indices),
/// its section, and how far down the page its top sits, in pixels.
#[derive(Clone, PartialEq, Debug)]
pub struct PageRow {
    pub section: MapSection,
    pub slots: std::ops::Range<usize>,
    pub top: f32,
    /// The section's first row, the one its heading sits on.
    pub first: bool,
}

/// Step 2's page laid out as the screen lays it out: each section a
/// heading over rows of [`CARD_COLUMNS`] cards, one under the other. The
/// sizes are the screen's own ([`ui::TILE_H`] and the rest), so what this
/// says is in view is what the window shows, and the scroll bar, the
/// cards hidden and the pictures drawn first all read it.
#[derive(Clone, PartialEq, Debug)]
pub struct MapPage {
    pub rows: Vec<PageRow>,
    /// Where each section's heading sits, in [`MapSection::ALL`] order.
    pub headings: [f32; 3],
    /// The whole page's height.
    pub height: f32,
}

impl MapPage {
    /// The page `slots` ([`map_slots`]) make. Each section's places are
    /// one run of them, in section order.
    pub fn of(slots: &[(MapSection, Slot)]) -> MapPage {
        use super::ui::{HEADING_H, SECTION_GAP, TILE_GAP, TILE_H};
        let mut rows = Vec::new();
        let mut headings = [0.0; 3];
        let mut y = 0.0;
        for section in MapSection::ALL {
            let places: Vec<usize> = slots
                .iter()
                .enumerate()
                .filter(|(_, (of, _))| *of == section)
                .map(|(index, _)| index)
                .collect();
            if section.index() > 0 {
                y += SECTION_GAP;
            }
            headings[section.index()] = y;
            y += HEADING_H;
            for (n, row) in places.chunks(CARD_COLUMNS).enumerate() {
                if n > 0 {
                    y += TILE_GAP;
                }
                rows.push(PageRow {
                    section,
                    slots: row[0]..row[row.len() - 1] + 1,
                    top: y,
                    first: n == 0,
                });
                y += TILE_H;
            }
        }
        MapPage {
            rows,
            headings,
            height: y,
        }
    }

    /// How many places the page has.
    pub fn places(&self) -> usize {
        self.rows.last().map_or(0, |row| row.slots.end)
    }

    /// The row place `slot` is on.
    pub fn row_of(&self, slot: usize) -> usize {
        self.rows
            .iter()
            .position(|row| row.slots.contains(&slot))
            .unwrap_or(self.rows.len().saturating_sub(1))
    }

    /// The scroll from `scroll` the least that brings `slot`'s row whole
    /// into view, its section's heading with it on the section's first
    /// row. The window's top is always a heading's top or a row's, so the
    /// window never opens on the empty bottom of a card it hides.
    pub fn scrolled(&self, scroll: f32, slot: usize) -> f32 {
        use super::ui::{TILE_H, VIEW_H};
        let Some(row) = self.rows.get(self.row_of(slot)) else {
            return 0.0;
        };
        let top = match row.first {
            true => self.headings[row.section.index()],
            false => row.top,
        };
        let bottom = row.top + TILE_H;
        if top < scroll {
            return top;
        }
        if bottom <= scroll + VIEW_H + 0.5 {
            return scroll;
        }
        // The first top far enough down to show the row's foot: there is
        // one, since the row's own reach is shorter than the window.
        self.headings
            .iter()
            .copied()
            .chain(self.rows.iter().filter(|row| !row.first).map(|row| row.top))
            .filter(|&line| line + VIEW_H + 0.5 >= bottom)
            .fold(top, f32::min)
    }

    /// Whether a thing `height` tall at `top` is whole in the window
    /// scrolled to `scroll`. Only whole ones are drawn: a card or a
    /// heading cut by the window's edge reads as a glitch.
    pub fn whole_in_view(scroll: f32, top: f32, height: f32) -> bool {
        top >= scroll - 0.5 && top + height <= scroll + super::ui::VIEW_H + 0.5
    }

    /// Whether place `slot`'s row is whole in view at `scroll`.
    pub fn slot_in_view(&self, scroll: f32, slot: usize) -> bool {
        self.rows
            .get(self.row_of(slot))
            .is_some_and(|row| Self::whole_in_view(scroll, row.top, super::ui::TILE_H))
    }

    /// Whether `section`'s heading is whole in view at `scroll`.
    pub fn heading_in_view(&self, scroll: f32, section: MapSection) -> bool {
        Self::whole_in_view(scroll, self.headings[section.index()], super::ui::HEADING_H)
    }

    /// The page as its scroll bar sees it, in pixels: the window's height
    /// at `scroll` down a page this tall.
    pub fn window(&self, scroll: f32) -> menu_ui::scroll_bar::ScrollWindow {
        menu_ui::scroll_bar::ScrollWindow {
            first: scroll,
            shown: super::ui::VIEW_H,
            total: self.height,
        }
    }

    /// The cursor on place `at` moved by `(dx, dy)`. A/D walk the places
    /// in order, from one row's end onto the next row and from one section
    /// into the next; W/S go a row up or down, across a section's edge
    /// too, to the same column, or the row's last card when it is shorter.
    /// Past the page's first or last place nothing moves.
    pub fn step(&self, at: usize, (dx, dy): (i32, i32)) -> usize {
        let mut at = at.min(self.places().saturating_sub(1));
        let moved = at as i64 + i64::from(dx);
        if dx != 0 && (0..self.places() as i64).contains(&moved) {
            at = moved as usize;
        }
        if dy != 0 {
            let row = self.row_of(at);
            let column = at - self.rows[row].slots.start;
            let target = usize::try_from(row as i64 + i64::from(dy))
                .ok()
                .and_then(|target| self.rows.get(target));
            if let Some(target) = target {
                at = (target.slots.start + column).min(target.slots.end - 1);
            }
        }
        at
    }
}

/// What a press on the map page asks for, beyond walking the cards.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapPress {
    Nothing,
    /// Esc: back where the page was opened from.
    Back,
    /// Enter on a card the table can play, which is now picked.
    Chosen(Card),
}

/// The map page's keys, wherever it is up: WASD walk the cards, picking
/// each one the table can play ([`card_open`]), Enter on one of those is
/// [`MapPress::Chosen`], Esc is [`MapPress::Back`]. The card saying how to
/// make a map is walked onto like the rest, and neither picked nor chosen.
/// What Back and Chosen lead to is the caller's: step 1 and step 3 on the
/// couch, the lobby and the match online.
/// The card Enter picks on the map page: the one under the cursor, when
/// the table can play it. `None` on the card saying how to make a map and
/// on a greyed card, where Enter does nothing, which the page's prompt
/// and a pad's A read before offering the key.
pub fn card_to_pick(
    menu: &MatchMenu,
    config: &MatchConfig,
    beaches: &CustomBeaches,
) -> Option<Card> {
    let slots = map_slots(beaches);
    match slots[menu.slot.min(slots.len() - 1)].1 {
        Slot::Card(card) if card_open(card, config.seats, beaches, menu.online) => Some(card),
        Slot::Card(_) | Slot::MakeOne => None,
    }
}

pub fn map_page_keys(
    keys: &ButtonInput<KeyCode>,
    menu: &mut MatchMenu,
    config: &mut MatchConfig,
    beaches: &CustomBeaches,
) -> MapPress {
    if keys.just_pressed(KeyCode::Escape) {
        return MapPress::Back;
    }
    let slots = map_slots(beaches);
    let at = menu.slot.min(slots.len() - 1);
    let open = |card: Card| card_open(card, config.seats, beaches, menu.online);
    if menu_ui::enter(keys) {
        return match card_to_pick(menu, config, beaches) {
            Some(card) => {
                pick_card(config, card, beaches);
                MapPress::Chosen(card)
            }
            None => MapPress::Nothing,
        };
    }
    let pressed = |codes: [KeyCode; 2]| codes.iter().any(|&code| keys.just_pressed(code));
    let dx = i32::from(pressed([KeyCode::KeyD, KeyCode::ArrowRight]))
        - i32::from(pressed([KeyCode::KeyA, KeyCode::ArrowLeft]));
    let dy = i32::from(pressed([KeyCode::KeyS, KeyCode::ArrowDown]))
        - i32::from(pressed([KeyCode::KeyW, KeyCode::ArrowUp]));
    if (dx, dy) == (0, 0) {
        return MapPress::Nothing;
    }
    let page = MapPage::of(&slots);
    menu.slot = page.step(at, (dx, dy));
    menu.scroll = page.scrolled(menu.scroll, menu.slot);
    if let Slot::Card(card) = slots[menu.slot].1
        && open(card)
    {
        pick_card(config, card, beaches);
    }
    MapPress::Nothing
}

/// Step 2: the map page's keys ([`map_page_keys`]), Enter on to the
/// players from a card the table fits, Esc back to step 1.
pub(super) fn map_keys(
    keys: &ButtonInput<KeyCode>,
    menu: &mut MatchMenu,
    config: &mut MatchConfig,
    beaches: &CustomBeaches,
) {
    match map_page_keys(keys, menu, config, beaches) {
        MapPress::Back => menu.step = Step::Game,
        MapPress::Chosen(_) => menu.step = Step::Players,
        MapPress::Nothing => {}
    }
}
