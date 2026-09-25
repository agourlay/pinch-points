//! What a spectator says to the table: a line typed and sent, with free
//! hands being the reason the job falls to them.

use super::*;

/// The line a spectator is typing, if one is open.
///
/// Its own resource rather than a field on the session: the session is
/// rebuilt between rounds and a half-typed line is not worth carrying
/// across, and the shell reads this to know the keyboard is spoken for.
#[derive(Resource, Default)]
pub struct SpectatorChat(pub Option<String>);

impl SpectatorChat {
    pub fn open(&self) -> bool {
        self.0.is_some()
    }
}

/// T opens a line, Enter says it, Esc drops it.
///
/// The key is read only for a spectator, so a player's T is still a player's
/// T: nothing here can take a letter out of a round being played.
pub fn spectator_chat_input(
    keys: Res<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    settings: Res<crate::app::settings::GameSettings>,
    mut chat: ResMut<SpectatorChat>,
    mut online: ResMut<Online>,
) {
    if !is_spectating(&online) {
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
    if me.is_empty() {
        // The hub refuses a line that resolves to no name, because an
        // empty name is the beach's own voice. Echoing it here anyway
        // would show the sender something nobody else was ever given. The
        // lobby asks for a name before it lets anyone watch, so this is
        // the player who went and cleared it again.
        return;
    }
    if let Some(session) = &mut online.0 {
        session.transport.send(NetMsg::chat(&me, &said));
        // Said to the table, and shown here too: the sender is a peer like
        // any other and its own datagram never comes back to it.
        session.heard.push((me, said));
    }
}
