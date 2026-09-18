//! The rail: what the people with no seat can do while a round is played.
//!
//! A table seats six and a room holds more, so at a real beach there are
//! people standing behind the chairs. They already watch the round in step
//! with everyone else; this is what they can say about it.
//!
//! Only a spectator types. A player has both hands on the keys and a
//! letter key would be taken out of the round; the rail has free hands,
//! which is the whole reason the job falls to them.

use crate::app::net::Online;
use crate::transport::NetMsg;
use bevy::prelude::*;

/// The line the rail is typing, if one is open.
///
/// Its own resource rather than a field on the session: the session is
/// rebuilt between rounds and a half-typed line is not worth carrying
/// across, and the shell reads this to know the keyboard is spoken for.
#[derive(Resource, Default)]
pub struct RailChat(pub Option<String>);

impl RailChat {
    pub fn open(&self) -> bool {
        self.0.is_some()
    }
}

/// Whether this peer is at the rail: online, in a round, holding no seat.
pub fn at_the_rail(online: &Online) -> bool {
    online
        .0
        .as_ref()
        .is_some_and(|session| session.session.watching())
}

/// T opens a line, Enter says it, Esc drops it.
///
/// The key is read only at the rail, so a player's T is still a player's
/// T: nothing here can take a letter out of a round being played.
pub fn rail_chat_input(
    keys: Res<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    settings: Res<crate::app::settings::GameSettings>,
    mut chat: ResMut<RailChat>,
    mut online: ResMut<Online>,
) {
    if !at_the_rail(&online) {
        // A seat was handed out mid-round, or the session ended under a
        // half-typed line. Either way the line is not going anywhere.
        chat.0 = None;
        typed.clear();
        return;
    }
    let Some(line) = &mut chat.0 else {
        // Not typing: the keystrokes belong to whatever else reads them,
        // and T is the way in.
        if caps.just_pressed(&keys, 'T') {
            chat.0 = Some(String::new());
            typed.clear();
        }
        return;
    };
    let said = crate::app::lobby::type_a_line(&mut typed, line);
    if keys.just_pressed(KeyCode::Escape) {
        chat.0 = None;
        return;
    }
    let Some(said) = said else {
        return;
    };
    chat.0 = None;
    if said.trim().is_empty() {
        return;
    }
    let me = settings.names[0].clone();
    if let Some(session) = &mut online.0 {
        session.transport.send(NetMsg::chat(&me, &said));
        // Said to the table, and shown here too: the sender is a peer like
        // any other and its own datagram never comes back to it.
        session.heard.push((me, said));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::net::OnlineSession;
    use crate::sim::{DEFAULT_DELAY, Lockstep};
    use crate::transport::{MatchTerms, UdpTransport};

    fn session(local: Option<u8>) -> OnlineSession {
        let step = match local {
            Some(seat) => Lockstep::new(seat, vec![0, 1], DEFAULT_DELAY),
            None => Lockstep::observer(vec![0, 1], DEFAULT_DELAY),
        };
        OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            step,
            2,
            MatchTerms::default(),
        )
    }

    /// The rail is the people with no seat, and only they type: a player's
    /// hands are on the keys and a letter taken for a chat line is a
    /// letter taken out of the round.
    #[test]
    fn only_a_seatless_peer_is_at_the_rail() {
        assert!(!at_the_rail(&Online::default()), "nobody is online at all");
        assert!(
            !at_the_rail(&Online(Some(session(Some(0))))),
            "a player holds a seat"
        );
        assert!(
            at_the_rail(&Online(Some(session(None)))),
            "a watcher holds none"
        );
    }
}
