//! Asking a peer to leave: K and a number, paged with Tab past the
//! card's rows, and the prompt that lists whom the numbers reach.

use super::*;
use crate::app::i18n::fill;

/// How many of the peers off the table one page of K numbers reaches: the
/// number keys past the card's last row.
const KICK_PAGE: usize = crate::app::menu_ui::NUMBER_KEYS.len() - MAX_PLAYERS;

/// How many pages of K numbers the peers off the table take, at least one:
/// what Tab turns through while the host picks whom to ask to leave.
pub(crate) fn kick_pages(peers: &PeerBook) -> usize {
    peers.off_the_table().count().div_ceil(KICK_PAGE).max(1)
}

/// The peers off the table ([`PeerBook::off_the_table`]) on page `page`,
/// by the K number each is asked to leave with: on from the card's last
/// row, as far as the number keys go. The prompt lists these and
/// [`ask_to_leave`] reads them, so the number shown beside a name is the
/// number that sends it away. A page past the last is the first again.
pub(crate) fn numbered_off_the_table(
    peers: &PeerBook,
    page: usize,
) -> impl Iterator<Item = (usize, usize, &crate::app::net::Peer)> {
    let numbers = MAX_PLAYERS + 1..=crate::app::menu_ui::NUMBER_KEYS.len();
    let skip = page % kick_pages(peers) * KICK_PAGE;
    peers
        .off_the_table()
        .skip(skip)
        .zip(numbers)
        .map(|((peer, row), at)| (at, peer, row))
}

/// What the host is shown while picking whom to ask to leave: how, and
/// past the card's rows, the onlookers and would-be players the card has
/// no row for on page `page`, each by the number that asks it, and which
/// page of how many when there is more than one. A watcher spamming the
/// chat, or a sixth player who dialled a full beach by its address, sat
/// on no row, and no number reached them; a beach holds forty peers, and
/// the number keys past the card three.
pub fn kick_prompt(tr: &crate::app::i18n::Tr, peers: &PeerBook, page: usize) -> String {
    let mut prompt = tr.lobby_kick_ask.to_string();
    for (at, _, peer) in numbered_off_the_table(peers, page) {
        // A peer that has not said what to call it yet is still somebody
        // to send away; its number alone has to do.
        let who = match peer.name.is_empty() {
            true => "?",
            false => peer.name.as_str(),
        };
        let sep = match at == MAX_PLAYERS + 1 {
            true => " | ",
            false => "  ",
        };
        prompt.push_str(&format!("{sep}{at}. {who}"));
    }
    let pages = kick_pages(peers);
    if pages > 1 {
        let shown = (page % pages + 1).to_string();
        prompt.push_str(" | ");
        prompt.push_str(&fill(
            tr.lobby_kick_more,
            &[("p", &shown), ("n", &pages.to_string())],
        ));
    }
    prompt
}

/// Everyone the host could ask to leave right now, by the K number that
/// asks them and the name they go by, in the order [`ask_to_leave`] reads
/// the numbers: the people at the table from 2 (the host is 1), the bots
/// that came straight here after them, and past the card's last row the
/// page of peers it has no row for. The list a pad picks from, where a
/// keyboard presses the number.
pub(crate) fn kick_rows(
    hosted: &Hosted,
    bots: &crate::app::bot_seats::BotSeats,
    tr: &crate::app::i18n::Tr,
) -> Vec<(u8, String)> {
    let page = hosted.kicking.unwrap_or(0);
    let called = |peer: &crate::app::net::Peer| match peer.name.is_empty() {
        true => "?".to_string(),
        false => peer.name.clone(),
    };
    let people: Vec<String> = hosted
        .peers
        .at_the_table()
        .map(|(_, p)| called(p))
        .collect();
    let arrived = bots
        .door
        .as_ref()
        .map_or_else(Vec::new, |door| door.arrived(tr));
    let at_table = people
        .into_iter()
        .chain(arrived.into_iter().map(|(_, who)| who))
        .zip(2..=MAX_PLAYERS);
    let off = numbered_off_the_table(&hosted.peers, page).map(|(at, _, peer)| (called(peer), at));
    at_table
        .chain(off)
        .map(|(who, at)| (at as u8, who))
        .collect()
}

/// The host asks the peer K number `at` to leave: it is told, and
/// forgotten, and everyone else hears why the chair is empty. The first
/// numbers are the table, the host (1) and the peers who came to play, in
/// order, which is how the AT THIS BEACH card numbers them; past its last
/// row, the peers it has no row for on the page the prompt shows, as it
/// numbers them ([`kick_prompt`]). Either way the pick is over.
pub(super) fn ask_to_leave(
    state: &mut LobbyState,
    bots: &mut crate::app::bot_seats::BotSeats,
    tr: &'static crate::app::i18n::Tr,
    at: usize,
) {
    let Some(hosted) = state.hosted_mut() else {
        return;
    };
    let page = hosted.kicking.take().unwrap_or(0);
    // The same people the card lists, numbered as it numbers them.
    let people: Vec<usize> = hosted.peers.at_the_table().map(|(peer, _)| peer).collect();
    // Past the people sit the bots that came straight here, as far as the
    // card has rows for them.
    if at <= MAX_PLAYERS
        && let Some(bot) = at.checked_sub(2 + people.len())
        && let Some(door) = &mut bots.door
        && let Some((id, who)) = door.arrived(tr).get(bot).cloned()
    {
        door.let_go(id);
        tell_sent_away(state, tr, &who);
        return;
    }
    let peer = match at > MAX_PLAYERS {
        true => numbered_off_the_table(&hosted.peers, page)
            .find(|&(number, _, _)| number == at)
            .map(|(_, peer, _)| peer),
        false => people.get(at.wrapping_sub(2)).copied(),
    };
    let Some(peer) = peer else {
        return;
    };
    let who = hosted
        .peers
        .get(peer)
        .map_or_else(String::new, |p| p.name.clone());
    // Told, forgotten, and kept out: a greeting already on its way would
    // otherwise seat it again as a ghost row (`UdpTransport::turn_away`).
    hosted.turn_away(peer);
    tell_sent_away(state, tr, &who);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// K and a number asks the peer on that row to leave, and past the
    /// card's rows the K numbers go on, through the peers it has no row
    /// for, as the prompt lists them. Capped at the card, a would-be player
    /// who dialled a full beach by its address and a watcher spamming the
    /// chat could not be sent away at all.
    #[test]
    fn the_prompt_numbers_whom_the_card_has_no_row_for() {
        let mut state = LobbyState {
            standing: hosting_lobby(),
            ..LobbyState::default()
        };
        if let Standing::Hosting(hosted) = &mut state.standing {
            hosted.peers = peers_named(&["A", "Gil", "B", "C", "D", "E", "F"], &[1]);
        }
        let mut bots = crate::app::bot_seats::BotSeats::default();
        let tr = &crate::app::i18n::EN;
        let names = |state: &LobbyState| {
            state.hosted().map_or_else(Vec::new, |hosted| {
                hosted
                    .peers
                    .iter()
                    .map(|peer| peer.name.clone())
                    .collect::<Vec<_>>()
            })
        };
        let prompt = |state: &LobbyState| {
            state
                .hosted()
                .map(|hosted| kick_prompt(tr, &hosted.peers, 0))
                .unwrap_or_default()
        };
        assert_eq!(
            prompt(&state),
            format!("{} | 7. Gil  8. F", tr.lobby_kick_ask),
            "the watcher and the sixth would-be player, in the order they came"
        );
        ask_to_leave(&mut state, &mut bots, tr, MAX_PLAYERS + 3);
        assert_eq!(names(&state).len(), 7, "nobody is nine");
        ask_to_leave(&mut state, &mut bots, tr, MAX_PLAYERS + 2);
        assert_eq!(names(&state), ["A", "Gil", "B", "C", "D", "E"], "8 was F");
        ask_to_leave(&mut state, &mut bots, tr, MAX_PLAYERS + 1);
        assert_eq!(names(&state), ["A", "B", "C", "D", "E"], "7 was Gil");
        assert_eq!(
            prompt(&state),
            tr.lobby_kick_ask,
            "and nobody left off the card"
        );
    }

    /// A beach holds forty peers and the number keys past the card reach
    /// three of them; Tab turns the page, the prompt names the page's
    /// three by the numbers that ask them, and those numbers ask exactly
    /// them. Past the last page is the first again.
    #[test]
    fn tab_turns_the_k_numbers_through_everybody_off_the_table() {
        let mut state = LobbyState {
            standing: hosting_lobby(),
            ..LobbyState::default()
        };
        // Five at the table and eleven watching.
        let names: Vec<String> = (0..16).map(|n| format!("P{n}")).collect();
        let named: Vec<&str> = names.iter().map(String::as_str).collect();
        let watching: Vec<usize> = (5..16).collect();
        if let Standing::Hosting(hosted) = &mut state.standing {
            hosted.peers = peers_named(&named, &watching);
        }
        let tr = &crate::app::i18n::EN;
        let pages = state.hosted().map_or(0, |hosted| kick_pages(&hosted.peers));
        assert_eq!(pages, 4, "eleven, three a page");
        let prompt = |state: &LobbyState, page| {
            state
                .hosted()
                .map(|hosted| kick_prompt(tr, &hosted.peers, page))
                .unwrap_or_default()
        };
        let more = |page: usize| fill(tr.lobby_kick_more, &[("p", &page.to_string()), ("n", "4")]);
        assert_eq!(
            prompt(&state, 2),
            format!(
                "{} | 7. P11  8. P12  9. P13 | {}",
                tr.lobby_kick_ask,
                more(3)
            )
        );
        assert_eq!(
            prompt(&state, 3),
            format!("{} | 7. P14  8. P15 | {}", tr.lobby_kick_ask, more(4))
        );
        assert_eq!(prompt(&state, 4), prompt(&state, 0), "round again");
        // K, Tab twice, 8: the second of the third page.
        if let Some(hosted) = state.hosted_mut() {
            hosted.kicking = Some(2);
        }
        let mut bots = crate::app::bot_seats::BotSeats::default();
        ask_to_leave(&mut state, &mut bots, tr, MAX_PLAYERS + 2);
        let left: Vec<String> = state.hosted().map_or_else(Vec::new, |hosted| {
            hosted.peers.iter().map(|peer| peer.name.clone()).collect()
        });
        assert_eq!(left.len(), 15);
        assert!(
            !left.contains(&"P12".to_string()),
            "P12 was asked: {left:?}"
        );
        assert_eq!(
            state.hosted().map(|hosted| hosted.kicking),
            Some(None),
            "and the pick is over"
        );
    }

    /// The card, the count and the K numbers are one list. With more
    /// would-be players than chairs, the table used to list all of them
    /// while the count stopped at five, and the bots that came straight
    /// here had no row left to sit in.
    #[test]
    fn the_table_lists_whom_k_numbers() {
        let mut state = LobbyState {
            standing: hosting_lobby(),
            ..LobbyState::default()
        };
        if let Standing::Hosting(hosted) = &mut state.standing {
            hosted.peers = peers_named(&["A", "B", "C", "D", "E", "F"], &[]);
        }
        let tr = &crate::app::i18n::EN;
        let table = state.roster(tr, "Host");
        assert_eq!(
            super::super::place_names(&table),
            ["Host", "A", "B", "C", "D", "E"],
            "a row a chair"
        );
        let aboard = state.hosted().map_or(0, Hosted::players_aboard);
        assert_eq!(aboard + 1, table.len(), "and the count is the rows");

        // K and the last row's number asks that row to go, by name.
        let mut bots = crate::app::bot_seats::BotSeats::default();
        ask_to_leave(&mut state, &mut bots, tr, table.len());
        let left = state.hosted().map(|hosted| {
            hosted
                .peers
                .iter()
                .map(|peer| peer.name.clone())
                .collect::<Vec<_>>()
        });
        assert_eq!(
            left.as_deref(),
            Some(&["A", "B", "C", "D", "F"].map(String::from)[..])
        );
    }
}
