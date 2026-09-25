//! Calling the winner: for the first half minute of a round a spectator
//! picks a seat, the host counts the calls, and the results card says who
//! called it.

use super::*;

/// How long the crowd has to call the winner: the first half minute of a
/// round, long enough to see who is doing well and too soon to know.
pub const PICKS_OPEN_FOR: u32 = 30 * crate::sim::TICKS_PER_SECOND;

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
#[allow(clippy::too_many_arguments)]
pub fn spectator_pick_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    time: Res<Time>,
    settings: Res<crate::app::settings::GameSettings>,
    (chat, events): (Res<SpectatorChat>, Res<SpectatorCard>),
    phase: Res<State<crate::app::VersusPhase>>,
    mut card: ResMut<PickCard>,
    mut online: ResMut<Online>,
    ui: Query<Entity, With<PickCardUi>>,
    mut resend: Local<f32>,
) {
    let shut = |commands: &mut Commands, card: &mut PickCard| {
        card.0 = false;
        for entity in &ui {
            commands.entity(entity).despawn();
        }
    };
    let playing = *phase.get() == crate::app::VersusPhase::Running;
    let open = online.0.as_ref().is_some_and(|s| s.stands.picks.open > 0);
    if !is_spectating(&online) || chat.open() || events.0 || !playing || !open {
        if card.0 {
            shut(&mut commands, &mut card);
        }
        return;
    }
    let Some(session) = &mut online.0 else {
        return;
    };
    if crate::app::lobby::once_a_second(&mut resend, time.delta_secs())
        && let Some(seat) = session.stands.my_pick
    {
        session.transport.send(NetMsg::SpectatorPick { seat });
    }
    if !card.0 {
        if caps.just_pressed(&keys, 'P') {
            card.0 = true;
            spawn_pick_card(&mut commands, &settings, session);
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || caps.just_pressed(&keys, 'P') {
        shut(&mut commands, &mut card);
        return;
    }
    const SEATS: [KeyCode; crate::sim::MAX_PLAYERS] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
    ];
    for (seat, key) in SEATS
        .into_iter()
        .enumerate()
        .take(usize::from(session.seats))
    {
        if keys.just_pressed(key) {
            let seat = seat as u8;
            session.stands.my_pick = Some(seat);
            session.transport.send(NetMsg::SpectatorPick { seat });
            shut(&mut commands, &mut card);
            return;
        }
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
    settings: Res<crate::app::settings::GameSettings>,
    mut online: ResMut<Online>,
) {
    let Some(seats) = online.0.as_ref().map(|s| s.seats) else {
        return;
    };
    // The same winners the standings crown, teams and all.
    let mode = crate::app::teams::in_play(&settings, &online, seats);
    let winners = crate::app::side_panels::leading_seats(sim.0.scores(), seats, mode);
    let Some(session) = &mut online.0 else {
        return;
    };
    let Some(seat) = session
        .stands
        .my_pick
        .filter(|_| session.session.watching())
    else {
        return;
    };
    let right = winners[usize::from(seat)];
    session.stands.calls.made += 1;
    session.stands.calls.right += u32::from(right);
    session.stands.last_call = Some((seat, right));
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
    let picks = match session.stands.picks_said {
        true => session.stands.picks,
        false => session.crowd_picks_now(),
    };
    if picks != session.stands.picks || again {
        session.stands.picks = picks;
        session.transport.send(NetMsg::CrowdPicks {
            picks: picks.counts,
            open: picks.open,
        });
    }
    if picks.open == 0 && !session.stands.picks_said {
        session.stands.picks_said = true;
        if picks.any() {
            let names = &session.names;
            let label = |seat: u8| crate::app::name_or_label(names, tr, seat);
            let line = crate::app::i18n::fill(tr.crowd_picked, &[("l", &picks.line(label))]);
            session.transport.send(NetMsg::chat("", &line));
            session.heard.push((String::new(), line));
        }
    }
}
