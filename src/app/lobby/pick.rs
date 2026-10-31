//! The host picking the map: once the host starts the match, the table is
//! frozen as the launch would deal it, and the map page step 2 of the
//! couch's setup shows comes up over the lobby, greyed for that table.
//! Enter on a card starts the match on it; Esc goes back to the lobby
//! card with nothing started and nobody dropped.
//!
//! An overlay inside the lobby rather than a screen of its own: the
//! lobby's systems (the beacon, the peers' greetings, the chat, the bots at
//! the door) go on running underneath it untouched, so nobody at the table
//! times out while the host makes up its mind.

use super::*;
use crate::app::match_setup::{Close, CustomBeaches, HostPage, MapPress, MatchMenu};
use crate::bots::listener::BotId;

/// The table the host froze to pick the map for, and what to put back.
///
/// Frozen by who sits at it, not by how many: the peers frozen are marked
/// on their own rows (`Peer::frozen`), and the bots by who they are, so a
/// newcomer sitting down while the host picks is never taken for one of
/// them, and one of them leaving is never hidden by a newcomer.
pub struct Picking {
    /// The people seated: the host and the peers frozen at the table.
    pub people: u8,
    /// The bots that came straight to this beach (route 2) seated behind
    /// them, by who they are.
    pub bots: Vec<BotId>,
    /// The map page up over the lobby, the setup to put back with it.
    pub(super) page: HostPage,
    /// A card was picked: the match goes on at the host's next tick.
    pub chosen: bool,
}

/// Who has gone from the frozen table, which calls the start off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Gone {
    /// A person frozen at it left, or asked to watch instead.
    Player,
    /// A bot frozen at it dropped from the door, or was let go.
    Bot,
}

/// Freeze the table as the launch would deal it now: each peer the seat
/// plan seats marked frozen on its row and everyone else not, and as many
/// of the `door` bots as there are chairs for behind them. Returns the
/// people (the host among them) and those bots.
fn freeze(peers: &mut PeerBook, door: &[(BotId, String)]) -> (u8, Vec<BotId>) {
    let plan = peers.seat_plan(peers.len());
    peers.freeze(&plan);
    let people = 1 + plan.iter().flatten().count() as u8;
    let bots = door
        .iter()
        .take(crate::app::net::chairs_for_bots(people))
        .map(|(id, _)| *id)
        .collect();
    (people, bots)
}

/// Whether somebody frozen at the table has gone since, and who: a person
/// whose row has gone (or who asked to watch), else a bot no longer at the
/// door. A newcomer is no change: it watches this match ([`frozen_plan`]).
pub(super) fn gone_from_the_table(
    picking: &Picking,
    peers: &PeerBook,
    door: &[(BotId, String)],
) -> Option<Gone> {
    let still = peers
        .iter()
        .filter(|row| row.frozen && !row.watches())
        .count();
    if still + 1 < usize::from(picking.people) {
        return Some(Gone::Player);
    }
    let here = |bot: &BotId| door.iter().any(|(id, _)| id == bot);
    match picking.bots.iter().all(here) {
        true => None,
        false => Some(Gone::Bot),
    }
}

/// The launch's seat plan while the table is frozen: the frozen peers
/// seated and nobody else ([`PeerBook::frozen_plan`]). What the roster
/// tells each peer of its chair reads too.
pub(super) fn frozen_plan(peers: &PeerBook) -> Vec<Option<u8>> {
    peers.frozen_plan(peers.len())
}

/// Freeze the table and put the map page up: the setup's seat count set to
/// the table's (so the cards grey by it), the cursor on the host's last
/// pick, and a fresh seed for the pictures, which the match is then
/// played on, so the beach previewed is the beach played.
pub(super) fn open_the_pick(
    state: &mut LobbyState,
    config: &mut MatchConfig,
    menu: &mut MatchMenu,
    beaches: &CustomBeaches,
    door: &[(BotId, String)],
) {
    let Some(hosted) = state.hosted_mut() else {
        return;
    };
    let (people, bots) = freeze(&mut hosted.peers, door);
    let seats = dealt(config, people + bots.len() as u8).seats;
    let before = config.clone();
    crate::app::match_setup::reseat(config, seats, beaches);
    hosted.picking = Some(Picking {
        people,
        bots,
        page: HostPage::open(before, config, menu, beaches),
        chosen: false,
    });
}

/// Pick the card under the page's cursor, as Enter there does, if the
/// table can play it: the dev hook's pick (`PINCH_LOBBY_PICK=go`).
pub(super) fn pick_the_card_here(
    state: &mut LobbyState,
    config: &mut MatchConfig,
    menu: &MatchMenu,
    beaches: &CustomBeaches,
) {
    let Some(picking) = state
        .hosted_mut()
        .and_then(|hosted| hosted.picking.as_mut())
    else {
        return;
    };
    if let Some(card) = crate::app::match_setup::card_to_pick(menu, config, beaches) {
        crate::app::match_setup::pick_card(config, card, beaches);
        picking.chosen = true;
    }
}

/// The map page's keys in the lobby: Esc back to the lobby card, Enter on
/// a card the table can play picks it and the match goes on. The Enter
/// that opened the page is let go. Returns whether the page was up, in
/// which case it had the keyboard.
pub(super) fn pick_keys(
    keys: &ButtonInput<KeyCode>,
    state: &mut LobbyState,
    config: &mut MatchConfig,
    menu: &mut MatchMenu,
    beaches: &CustomBeaches,
) -> bool {
    let Some(picking) = state
        .hosted_mut()
        .and_then(|hosted| hosted.picking.as_mut())
    else {
        return false;
    };
    if picking.page.just_opened() || picking.chosen {
        return true;
    }
    match crate::app::match_setup::map_page_keys(keys, menu, config, beaches) {
        MapPress::Nothing => {}
        MapPress::Chosen(_) => picking.chosen = true,
        MapPress::Back => {
            if let Some(picking) = state.hosted_mut().and_then(|hosted| hosted.picking.take()) {
                picking.page.close(Close::PutBack, config, menu, beaches);
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A book of `rows` peers, the ones in `watching` asking to watch.
    fn book(rows: usize, watching: &[usize]) -> PeerBook {
        let mut peers = PeerBook::default();
        peers.reach(rows);
        for &at in watching {
            peers.row(at).watch = true;
        }
        peers
    }

    /// The frozen are who the plan seated, and the launch seats them alone,
    /// in order: a watcher who sat down to play, and a newcomer, watch.
    #[test]
    fn the_plan_seats_who_was_frozen_and_nobody_else() {
        let mut peers = book(3, &[1]);
        let (people, bots) = freeze(&mut peers, &[]);
        assert_eq!((people, bots.len()), (3, 0));
        // The watcher sits down to play, and somebody new turns up.
        peers.row(1).watch = false;
        peers.reach(4);
        assert_eq!(frozen_plan(&peers), [Some(1), None, Some(2), None]);
    }

    /// One frozen leaving is a player gone, whoever sat down meanwhile:
    /// frozen were the host and two, one of those two left and a
    /// newcomer came, which a count would have read as the same table.
    #[test]
    fn a_frozen_player_leaving_is_seen_past_a_newcomer() {
        let mut peers = book(2, &[]);
        let (people, bots) = freeze(&mut peers, &[]);
        let picking = Picking {
            people,
            bots,
            page: HostPage::settled(MatchConfig::default()),
            chosen: false,
        };
        peers.reach(3);
        assert_eq!(gone_from_the_table(&picking, &peers, &[]), None);
        peers.forget(0);
        assert_eq!(peers.len(), 2, "the newcomer is in the count again");
        assert_eq!(
            gone_from_the_table(&picking, &peers, &[]),
            Some(Gone::Player)
        );
        assert_eq!(frozen_plan(&peers), [Some(1), None]);
    }

    /// The frozen table as a pick opened on it: these peers, these bots.
    fn picking(peers: &mut PeerBook, door: &[(BotId, String)]) -> Picking {
        let (people, bots) = freeze(peers, door);
        Picking {
            people,
            bots,
            page: HostPage::settled(MatchConfig::default()),
            chosen: false,
        }
    }

    /// The host, two peers and three bots at the door, frozen; a fourth
    /// person sitting down is no change, though the launch would now seat
    /// a bot fewer, and neither is a bot coming to the door. The bot count
    /// read again against the grown table called the start off as "a
    /// player left".
    #[test]
    fn a_newcomer_never_calls_the_start_off() {
        let door: Vec<(BotId, String)> = (1..=3).map(|id| (id, format!("Bot {id}"))).collect();
        let mut peers = book(2, &[]);
        let frozen = picking(&mut peers, &door);
        assert_eq!((frozen.people, frozen.bots.len()), (3, 3));
        peers.reach(3);
        let mut more = door.clone();
        more.push((4, "Bot 4".into()));
        assert_eq!(gone_from_the_table(&frozen, &peers, &more), None);
        assert_eq!(frozen_plan(&peers), [Some(1), Some(2), None]);
    }

    /// A bot frozen at the table dropping from its door is said to be a
    /// bot, not a player: it keeps its chair at the door for a while, but
    /// the launch seats only the bots still there.
    #[test]
    fn a_frozen_bot_dropping_is_a_bot_gone() {
        let door: Vec<(BotId, String)> = (1..=2).map(|id| (id, format!("Bot {id}"))).collect();
        let mut peers = book(1, &[]);
        let frozen = picking(&mut peers, &door);
        assert_eq!(
            gone_from_the_table(&frozen, &peers, &door[..1]),
            Some(Gone::Bot)
        );
        // And a person's leaving is said first, when both went.
        peers.forget(0);
        assert_eq!(
            gone_from_the_table(&frozen, &peers, &door[..1]),
            Some(Gone::Player)
        );
    }
}
