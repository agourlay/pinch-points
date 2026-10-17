//! Calling the winner: for the first half minute of a round a spectator
//! picks a seat, the host counts the calls, and the results card says who
//! called it.

use super::*;

/// How long the crowd has to call the winner: the first half minute of a
/// round, long enough to see who is doing well and too soon to know.
pub const PICKS_OPEN_FOR: u32 = 30 * crate::sim::TICKS_PER_SECOND;

/// The spectators' calls for the winner as one peer holds them: this
/// round's count and its own call, and the record of every call it made.
#[derive(Default)]
pub struct WinnerCalls {
    /// The host's word on the crowd's calls for the winner (on the host,
    /// what it is about to say), like [`TideVote::crowd`].
    pub picks: Picks,
    /// Host: whether the closing count has gone to the feed this round.
    pub picks_said: bool,
    /// A spectator's own call this round, if it made one.
    pub my_pick: Option<u8>,
    /// A spectator's calls over the whole session, for the results card.
    pub calls: Calls,
    /// How this round's call went, once the tide is in: the seat, and
    /// whether it won.
    pub last_call: Option<(u8, bool)>,
}

impl WinnerCalls {
    /// A new round: every call is for one round, while the record of them
    /// is the session's.
    pub fn new_round(&mut self) {
        self.picks = Picks::default();
        self.picks_said = false;
        self.my_pick = None;
        self.last_call = None;
    }
}

/// What the host says about the crowd's calls for the winner.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Picks {
    /// How many spectators have called each seat.
    pub counts: [u8; crate::sim::MAX_PLAYERS],
    /// Whole seconds left to call one, zero once the calls are in.
    pub open: u8,
}

impl Picks {
    /// Whether anybody called anyone.
    pub fn any(&self) -> bool {
        self.counts.iter().any(|&n| n > 0)
    }

    /// "Maya ×2 · Theo ×1": the seats called, most called first, and the
    /// lower seat first on a tie so every screen lists them alike.
    pub fn line(&self, name: impl Fn(u8) -> String) -> String {
        let mut called: Vec<(u8, u8)> = (0..crate::sim::MAX_PLAYERS as u8)
            .map(|seat| (seat, self.counts[usize::from(seat)]))
            .filter(|&(_, n)| n > 0)
            .collect();
        called.sort_by_key(|&(seat, n)| (std::cmp::Reverse(n), seat));
        called
            .into_iter()
            .map(|(seat, n)| format!("{} ×{n}", name(seat)))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// A spectator's calls over the session: how many came in, of how many.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Calls {
    pub right: u32,
    pub made: u32,
}

/// The seats to call, while a spectator has the list open.
#[derive(Component)]
pub struct PickCardUi;

/// Whether the list of seats to call is on screen.
#[derive(Resource, Default)]
pub struct PickCard(pub bool);

/// Call the winner: P opens the seats, a number calls one.
///
/// Open while the host still takes calls, and a call already made can be
/// changed until then. Repeated to the host once a second while the calls
/// are open, since a pick is one datagram and UDP owes nobody that.
pub fn spectator_pick_input(
    mut card: ListCard<PickCardUi, PickCard>,
    mut keyboard: crate::app::keycaps::KeyboardMut,
    time: Res<Time>,
    settings: Res<crate::app::settings::GameSettings>,
    watching: Watching<SpectatorCard>,
    mut online: ResMut<Online>,
    mut resend: Local<f32>,
) {
    let open = online
        .0
        .as_ref()
        .is_some_and(|s| s.stands.winner.picks.open > 0);
    if !is_spectating(&online) || !watching.free() || watching.other_up() || !open {
        if card.is_up() {
            card.shut();
        }
        return;
    }
    let Some(session) = &mut online.0 else {
        return;
    };
    if crate::app::lobby::once_a_second(&mut resend, time.delta_secs())
        && let Some(seat) = session.stands.winner.my_pick
    {
        session.transport.send(NetMsg::SpectatorPick { seat });
    }
    if !card.is_up() {
        if keyboard.global('P') {
            spawn_pick_card(card.raise(), &settings, session);
        }
        return;
    }
    if keyboard.keys.just_pressed(KeyCode::Escape) || keyboard.global('P') {
        card.shut();
        // Or the pause card, behind it in the frame, opens on the same Esc.
        crate::app::menu_ui::spend(&mut keyboard.keys, &[KeyCode::Escape]);
        return;
    }
    if let Some(seat) =
        crate::app::menu_ui::number_pressed(&keyboard.keys, usize::from(session.seats))
    {
        let seat = seat as u8;
        session.stands.winner.my_pick = Some(seat);
        session.transport.send(NetMsg::SpectatorPick { seat });
        card.shut();
    }
}

fn spawn_pick_card(
    commands: &mut Commands,
    settings: &crate::app::settings::GameSettings,
    session: &crate::app::net::OnlineSession,
) {
    let tr = settings.tr();
    let rows = (0..session.seats).map(|seat| {
        let name = crate::app::name_or_label(&session.names, tr, seat);
        (
            format!("{}  {name}", seat + 1),
            crate::app::palette::player_color(seat),
        )
    });
    spawn_list_card(commands, PickCardUi, tr.spectator_pick_title, rows);
}

/// Score a spectator's call as the tide comes in, for the card to say.
pub fn score_the_call(
    sim: Res<crate::app::Sim>,
    local: crate::app::teams::LocalScoring,
    mut online: ResMut<Online>,
) {
    let Some(seats) = online.0.as_ref().map(|s| s.seats) else {
        return;
    };
    // The same winners the standings crown, teams and all.
    // The pure rule rather than `Scoring`, which reads the session this
    // writes; a call is only ever made on a live round.
    let mode = crate::app::teams::in_play(local.mode(), &online, None, seats);
    let winners = crate::app::seating::leading_seats(sim.0.scores(), seats, mode);
    let Some(session) = &mut online.0 else {
        return;
    };
    let Some(seat) = session
        .stands
        .winner
        .my_pick
        .filter(|_| session.session.watching())
    else {
        return;
    };
    let right = winners[usize::from(seat)];
    session.stands.winner.calls.made += 1;
    session.stands.winner.calls.right += u32::from(right);
    session.stands.winner.last_call = Some((seat, right));
}

/// Host: tell everyone where the calls for the winner stand, when they
/// change and on the crowd's clock besides (`again`), and read the count
/// out to the feed once when they close.
///
/// Said after they close as well: the closing count is one datagram too,
/// and a spectator that missed it would be offered a call for ever.
pub(super) fn say_the_calls(
    session: &mut crate::app::net::OnlineSession,
    tr: &crate::app::i18n::Tr,
    again: bool,
) {
    // Counted until they close and then held, so a spectator who leaves
    // afterwards does not take their call out of a count the feed has
    // already read out and the results card will quote.
    let picks = match session.stands.winner.picks_said {
        true => session.stands.winner.picks,
        false => session.crowd_picks_now(),
    };
    if picks != session.stands.winner.picks || again {
        session.stands.winner.picks = picks;
        session.transport.send(NetMsg::CrowdPicks {
            picks: picks.counts,
            open: picks.open,
        });
    }
    if picks.open == 0 && !session.stands.winner.picks_said {
        session.stands.winner.picks_said = true;
        if picks.any() {
            let names = &session.names;
            let label = |seat: u8| crate::app::name_or_label(names, tr, seat);
            let line = crate::app::i18n::fill(tr.crowd_picked, &[("l", &picks.line(label))]);
            session.transport.send(NetMsg::chat("", &line));
            session.heard.push((String::new(), line));
        }
    }
}
