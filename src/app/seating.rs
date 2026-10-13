//! Who sits where: which seats lead, who is played at this screen, and
//! how a seat is labelled. Read by the panels, the results card, the
//! beach and the bookkeeping that follows a round alike.

use crate::app::net::Online;
use crate::app::palette;
use crate::app::teams::TeamMode;
use crate::app::{Controllers, Playback, SeatController, SeatNames, Seats};
use crate::sim::MAX_PLAYERS;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// Index of the strictly-largest nonzero entry, if it is unique. The
/// shared core of "who is winning" for rounds, series, and panels.
pub fn unique_max<T: Copy + Ord + Default>(values: &[T]) -> Option<usize> {
    let best = values.iter().max().copied()?;
    if best == T::default() || values.iter().filter(|&&v| v == best).count() != 1 {
        return None;
    }
    values.iter().position(|&v| v == best)
}

/// Which seats currently lead: the unique top scorer (or, in 2v2, both
/// members of the strictly leading team). Nobody leads at zero or in a tie.
pub fn leading_seats(
    scores: &[u32; MAX_PLAYERS],
    seats: u8,
    mode: TeamMode,
) -> [bool; MAX_PLAYERS] {
    let mut leaders = [false; MAX_PLAYERS];
    if mode == TeamMode::Solo {
        if let Some(winner) = unique_max(&scores[..usize::from(seats.max(2))]) {
            leaders[winner] = true;
        }
        return leaders;
    }
    let totals = crate::app::teams::team_scores(scores, seats, mode);
    if let Some(team) = unique_max(&totals) {
        for seat in mode.seats_of(team as u8, seats) {
            leaders[usize::from(seat)] = true;
        }
    }
    leaders
}

/// Who sits where: the table, which chairs the AI holds and what everyone
/// is called, and whether this screen is a peer's, a recording's or the
/// keyboard's. Read together wherever a seat is labelled or a mark is
/// shown only to the people it is for.
#[derive(SystemParam)]
pub struct Seating<'w> {
    pub seats: Res<'w, Seats>,
    pub controllers: Res<'w, Controllers>,
    pub names: Res<'w, SeatNames>,
    pub online: Res<'w, Online>,
    pub playback: Res<'w, Playback>,
}

impl Seating<'_> {
    /// The seat at this keyboard: see [`local_seat`].
    pub fn local(&self) -> Option<u8> {
        local_seat(&self.online, self.playback.0.is_some())
    }

    /// Whether `seat` is played at this screen: see [`played_here`].
    pub fn played_here(&self, seat: u8) -> bool {
        played_here(
            self.local(),
            self.online.0.is_some(),
            &self.controllers,
            seat,
        )
    }

    /// A seat's name and its "(you)" or "(AI)" tag, apart: a chip clips a
    /// long name and never its tag.
    pub fn name_and_tag(&self, tr: &crate::app::i18n::Tr, seat: u8) -> (String, &'static str) {
        let tag = seat_tag(tr, &self.controllers, self.local(), seat);
        (self.names.label(tr, seat), tag)
    }
}

/// The seat the person at this keyboard is playing, and `None` when
/// nobody is: a spectator at somebody else's beach, or a recording.
///
/// Offline the answer is seat 0, because the person at the keyboard is
/// always P1 there. [`seat_tag`] used to make that guess itself, from a
/// local seat of `None`, and a spectator has one of those too: every
/// watcher on a beach was shown the host's chair marked as its own, on
/// the panels and on the results card both.
pub fn local_seat(online: &Online, playback_active: bool) -> Option<u8> {
    match &online.0 {
        Some(session) => session.session.seat(),
        // A recording is watched, not played, so nobody at this keyboard
        // holds a chair in it either.
        None if playback_active => None,
        None => Some(0),
    }
}

/// Whether `seat` is played at this screen: offline every seat the AI
/// does not hold, since everyone sharing the keyboard and the pads is here;
/// online only the chair this peer was dealt; and nobody's while watching,
/// a recording or somebody else's beach. `local` is [`local_seat`]'s.
///
/// What the next-to-go mark is shown for. A table of AI is always at its
/// cap, and marking their posts set half the beach wobbling for nobody.
pub fn played_here(local: Option<u8>, online: bool, controllers: &Controllers, seat: u8) -> bool {
    match local {
        None => false,
        Some(mine) if online => mine == seat,
        Some(_) => controllers.0.get(usize::from(seat)) == Some(&SeatController::Local),
    }
}

/// The "(you)" / "(AI)" tag for a seat, shared by the panels and the
/// results card.
pub fn seat_tag(
    tr: &crate::app::i18n::Tr,
    controllers: &Controllers,
    local: Option<u8>,
    seat: u8,
) -> &'static str {
    if controllers.ai(usize::from(seat)).is_some() {
        tr.tag_ai
    } else if local == Some(seat) {
        tr.tag_you
    } else {
        ""
    }
}

/// The bot tag before a name: the robot's head in the seat's colour, `px`
/// square, drawn in the line of text as one of its spans. The game sets it
/// from what holds the seat, never from the name, so a bot that calls
/// itself after a friend still wears it.
///
/// In the line rather than a node beside it (Bevy 0.20's `InlineImage`):
/// it sits on the text's own line, clips with it, and needs no row of
/// nodes around the text to hold it.
pub fn robot_inline(art: &crate::app::art::Art, seat: u8, px: f32) -> InlineImage {
    InlineImage {
        image: art.robot.clone(),
        color: palette::player_color(seat).lighter(0.3),
        width: Some(px),
        height: Some(px),
        ..default()
    }
}

/// The text that follows the robot, or stands alone without one: a space
/// apart from it when there is one.
pub fn beside_the_robot(bot: bool, text: &str) -> String {
    match bot {
        true => format!(" {text}"),
        false => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaders_need_a_unique_nonzero_top() {
        assert_eq!(
            leading_seats(&[0, 0, 0, 0, 0, 0], 4, TeamMode::Solo),
            [false; MAX_PLAYERS]
        );
        assert_eq!(
            leading_seats(&[1, 5, 2, 0, 0, 0], 4, TeamMode::Solo),
            [false, true, false, false, false, false]
        );
        assert_eq!(
            leading_seats(&[5, 5, 0, 0, 0, 0], 2, TeamMode::Solo),
            [false; MAX_PLAYERS]
        );
        // Teams: both members of the strictly leading team are crowned.
        assert_eq!(
            leading_seats(&[1, 2, 4, 0, 0, 0], 4, TeamMode::Pairs),
            [false, false, true, true, false, false]
        );
        // Six seats in trios: the mirror-image half that leads is crowned.
        assert_eq!(
            leading_seats(&[9, 1, 9, 1, 9, 1], 6, TeamMode::Trios),
            [true, false, true, false, true, false]
        );
    }
}

#[cfg(test)]
mod seat_tag_tests {
    use super::*;
    use crate::app::i18n::EN;
    use crate::sim::{DEFAULT_DELAY, Lockstep};
    use crate::transport::{MatchTerms, UdpTransport};

    fn session(local: Option<u8>) -> crate::app::net::OnlineSession {
        let step = match local {
            Some(seat) => Lockstep::new(seat, vec![0, 1], DEFAULT_DELAY),
            None => Lockstep::observer(vec![0, 1], DEFAULT_DELAY),
        };
        crate::app::net::OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            step,
            2,
            MatchTerms::default(),
        )
    }

    /// Who "(you)" belongs to, which is not the same question as "is
    /// there a local seat".
    ///
    /// Offline the person at the keyboard is always P1, and that guess
    /// used to be made from a local seat of `None`. A spectator has one of
    /// those too, so every watcher on a beach was shown the host's chair
    /// marked as its own, on the panels and the results card both.
    #[test]
    fn a_spectator_is_nobody_at_the_table_rather_than_seat_zero() {
        assert_eq!(
            local_seat(&Online::default(), false),
            Some(0),
            "offline, the keyboard is P1"
        );
        assert_eq!(
            local_seat(&Online::default(), true),
            None,
            "a recording is watched, not played"
        );
        assert_eq!(
            local_seat(&Online(Some(session(Some(1)))), false),
            Some(1),
            "online, whatever chair the host dealt"
        );
        assert_eq!(
            local_seat(&Online(Some(session(None))), false),
            None,
            "and a spectator holds none"
        );

        let controllers = Controllers::default();
        let tag = |local, seat| seat_tag(&EN, &controllers, local, seat);
        assert_eq!(tag(Some(0), 0), EN.tag_you);
        assert_eq!(tag(Some(0), 1), "");
        assert_eq!(tag(None, 0), "", "nobody's chair is the spectator's");
        assert_eq!(tag(None, 1), "");
    }
}
