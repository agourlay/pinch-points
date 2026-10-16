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
    /// Who is at this screen: see [`Viewer::of`].
    pub fn viewer(&self) -> Viewer {
        Viewer::of(&self.online, self.playback.0.is_some())
    }

    /// Whether `seat` is played at this screen: see [`played_here`].
    pub fn played_here(&self, seat: u8) -> bool {
        played_here(self.viewer(), &self.controllers, seat)
    }

    /// A seat's name and its "(you)" or "(AI)" tag, apart: a chip clips a
    /// long name and never its tag.
    pub fn name_and_tag(&self, tr: &crate::app::i18n::Tr, seat: u8) -> (String, &'static str) {
        let tag = seat_tag(tr, &self.controllers, self.viewer(), seat);
        (self.names.label(tr, seat), tag)
    }
}

/// Who is in front of this screen, and so whose chair, if any, is theirs.
///
/// One answer for the four ways of being at a beach. It was an
/// `Option<u8>` from `local_seat`, whose `None` meant a spectator and a
/// recording alike, with every caller asking the session and the playback
/// again to tell them apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Viewer {
    /// A recording playing back: watched, not played, so nobody here holds
    /// a chair in it.
    Replay,
    /// Online, at somebody else's beach without a chair.
    Spectator,
    /// Online, in the chair this peer was dealt.
    Peer(u8),
    /// Offline: everyone sharing the keyboard and the pads.
    Table,
}

impl Viewer {
    /// Who is at this screen, with an online session the first word on it:
    /// a peer is its seat or a spectator whatever else is loaded, and only
    /// offline does a recording playing make this a replay.
    ///
    /// The header asks the other way round, recording first (see
    /// `hud::text::header_viewer`). The two can only disagree with a
    /// recording playing under a live session, and each keeps the answer
    /// it has always given there.
    pub fn of(online: &Online, playback_active: bool) -> Viewer {
        match &online.0 {
            Some(session) => match session.session.seat() {
                Some(seat) => Viewer::Peer(seat),
                None => Viewer::Spectator,
            },
            None if playback_active => Viewer::Replay,
            None => Viewer::Table,
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
    pub fn seat(self) -> Option<u8> {
        match self {
            Viewer::Replay | Viewer::Spectator => None,
            Viewer::Peer(seat) => Some(seat),
            Viewer::Table => Some(0),
        }
    }
}

/// Whether `seat` is played at this screen: offline every seat the AI
/// does not hold, since everyone sharing the keyboard and the pads is here;
/// online only the chair this peer was dealt; and nobody's while watching,
/// a recording or somebody else's beach.
///
/// What the next-to-go mark is shown for. A table of AI is always at its
/// cap, and marking their posts set half the beach wobbling for nobody.
pub fn played_here(viewer: Viewer, controllers: &Controllers, seat: u8) -> bool {
    match viewer {
        Viewer::Replay | Viewer::Spectator => false,
        // Not when this peer joined as a bot: the chair is the bot's to
        // play, and the person here only watches it.
        Viewer::Peer(mine) => {
            mine == seat && controllers.0.get(usize::from(seat)) != Some(&SeatController::Bot)
        }
        Viewer::Table => controllers.0.get(usize::from(seat)) == Some(&SeatController::Local),
    }
}

/// The "(you)" / "(AI)" tag for a seat, shared by the panels and the
/// results card.
pub fn seat_tag(
    tr: &crate::app::i18n::Tr,
    controllers: &Controllers,
    viewer: Viewer,
    seat: u8,
) -> &'static str {
    let held_by = controllers.0.get(usize::from(seat));
    if controllers.ai(usize::from(seat)).is_some() {
        tr.tag_ai
    } else if viewer.seat() == Some(seat) && held_by != Some(&SeatController::Bot) {
        // A seat a bot drives wears the robot already, and is the bot's,
        // not this person's, even when it is the chair this peer was dealt.
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
            Viewer::of(&Online::default(), false).seat(),
            Some(0),
            "offline, the keyboard is P1"
        );
        assert_eq!(
            Viewer::of(&Online::default(), true).seat(),
            None,
            "a recording is watched, not played"
        );
        assert_eq!(
            Viewer::of(&Online(Some(session(Some(1)))), false).seat(),
            Some(1),
            "online, whatever chair the host dealt"
        );
        assert_eq!(
            Viewer::of(&Online(Some(session(None))), false).seat(),
            None,
            "and a spectator holds none"
        );

        let controllers = Controllers::default();
        let tag = |viewer, seat| seat_tag(&EN, &controllers, viewer, seat);
        assert_eq!(tag(Viewer::Table, 0), EN.tag_you);
        assert_eq!(tag(Viewer::Table, 1), "");
        assert_eq!(tag(Viewer::Peer(1), 1), EN.tag_you);
        assert_eq!(tag(Viewer::Peer(1), 0), "");
        let none = "nobody's chair is the spectator's";
        assert_eq!(tag(Viewer::Spectator, 0), "", "{none}");
        assert_eq!(tag(Viewer::Spectator, 1), "");
        assert_eq!(tag(Viewer::Replay, 0), "", "nor a recording's");
    }

    /// The four ways of being at a beach, and which wins when a recording
    /// plays under a live session: the session, for every seat question.
    /// The header answers that one state the other way (it titles the
    /// screen by the recording); `hud::text` pins its side.
    #[test]
    fn the_session_decides_who_sits_here_even_over_a_recording() {
        assert_eq!(Viewer::of(&Online::default(), false), Viewer::Table);
        assert_eq!(Viewer::of(&Online::default(), true), Viewer::Replay);
        let seated = Online(Some(session(Some(1))));
        let watching = Online(Some(session(None)));
        for playback in [false, true] {
            assert_eq!(Viewer::of(&seated, playback), Viewer::Peer(1));
            assert_eq!(Viewer::of(&watching, playback), Viewer::Spectator);
        }
    }

    /// The next-to-go mark: every human chair offline, only your own
    /// online, and none for a watcher of either kind.
    #[test]
    fn played_here_is_each_viewers_own_hands() {
        let mut controllers = Controllers::default();
        controllers.0[1] = SeatController::Ai(crate::sim::BotLevel::default());
        let here = |viewer, seat| played_here(viewer, &controllers, seat);
        assert!(here(Viewer::Table, 0));
        assert!(!here(Viewer::Table, 1), "the AI's chair");
        assert!(
            here(Viewer::Peer(1), 1),
            "a peer's own, whoever holds it here"
        );
        assert!(!here(Viewer::Peer(1), 0));
        for seat in 0..2 {
            assert!(!here(Viewer::Spectator, seat));
            assert!(!here(Viewer::Replay, seat));
        }
    }

    /// A peer that joined as a bot (route 1) is dealt a chair the bot
    /// plays: it wears the robot, and is neither "(you)" nor marked as
    /// this person's next to go.
    #[test]
    fn a_chair_this_peers_bot_plays_is_not_yours() {
        let mut controllers = Controllers::default();
        controllers.0[1] = SeatController::Bot;
        assert_eq!(seat_tag(&EN, &controllers, Viewer::Peer(1), 1), "");
        assert!(!played_here(Viewer::Peer(1), &controllers, 1));
    }
}
