//! The room's notices, worded by whoever reads them.
//!
//! The host sends what happened ([`Notice`]): who turned up, who left, what
//! the spectators called. Each peer fills its own template from it, so a
//! French joiner at a Japanese host reads the room in French, event names
//! and seat labels included, beside a banner its own game wrote in French.

use crate::app::i18n::{Tr, fill};
use crate::transport::{Notice, chat_fits, name_from_wire};

/// What `notice` says, in `tr`'s language, `label` naming a seat. Kept to
/// one chat line ([`chat_fits`]), the line the feeds are laid out for, as
/// it was when it crossed the wire as one: the crowd's calls drop the
/// least called seats that do not fit rather than cutting the last one
/// off.
pub fn notice_line(tr: &Tr, notice: &Notice, label: impl Fn(u8) -> String) -> String {
    let named = |template: &str, name| fill(template, &[("p", &name_from_wire(name))]);
    match notice {
        Notice::Joined {
            name,
            watching: false,
        } => named(tr.lobby_joined, name),
        Notice::Joined {
            name,
            watching: true,
        } => named(tr.lobby_watching_joined, name),
        Notice::Left { name } => named(tr.lobby_left, name),
        Notice::SentAway { name } => named(tr.lobby_kicked_feed, name),
        Notice::PickCalledOff { bot: false } => tr.lobby_pick_called_off.to_string(),
        Notice::PickCalledOff { bot: true } => tr.lobby_pick_called_off_bot.to_string(),
        Notice::Called { event } => {
            let event = tr.events.get(usize::from(*event)).copied().unwrap_or("");
            fill(tr.spectator_called, &[("e", event)])
        }
        Notice::CrowdPicked { picks } => {
            let picks = crate::app::spectators::Picks {
                counts: *picks,
                open: 0,
            };
            let said = |l: &str| fill(tr.crowd_picked, &[("l", l)]);
            said(&picks.line_within(label, |l| chat_fits(&said(l))))
        }
    }
}
