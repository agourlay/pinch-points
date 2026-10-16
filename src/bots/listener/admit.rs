//! Admission and its defences: who may register, the cap on connections
//! still registering, and the lockout an address earns by guessing keys.

use super::allowance::Allowance;
use super::wire::{Wire, attach, ping, routes_of, send};
use super::{
    Admission, Bot, BotId, BotInfo, Event, GameMsg, Registry, Shared, emit, lock, owner_key,
};
use crate::bots::connstr::{Key, Token};
use crate::bots::protocol::{PROTOCOL, Register};
use crate::sim::PlayerId;
use serde_json::json;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::mpsc::Sender;
use std::sync::{Arc, MutexGuard};
use std::time::{Duration, Instant};

/// Wrong keys from one address before that address is turned away.
const WRONG_KEYS: u32 = 3;
/// How long an address that guessed badly is turned away.
const LOCKOUT: Duration = Duration::from_secs(60);
/// Connections that have not registered yet, in all and from one address.
/// Each holds a thread for up to [`REGISTER_WITHIN`](super::wire::REGISTER_WITHIN), so without a cap a
/// few hundred silent connections would use the machine up.
const PENDING: usize = 64;
const PENDING_PER_ADDR: usize = 8;

/// A connection's place among those waiting to register, given back when
/// it registers or goes.
pub(super) struct Pending<'a> {
    shared: &'a Shared,
    ip: IpAddr,
}

impl<'a> Pending<'a> {
    pub(super) fn take(shared: &'a Shared, ip: IpAddr) -> Option<Pending<'a>> {
        let mut registry = lock(&shared.registry);
        let all: usize = registry.pending.values().sum();
        let here = registry.pending.entry(ip).or_insert(0);
        if all >= PENDING || *here >= PENDING_PER_ADDR {
            if *here == 0 {
                registry.pending.remove(&ip);
            }
            return None;
        }
        *here += 1;
        Some(Pending { shared, ip })
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        let mut registry = lock(&self.shared.registry);
        if let Some(here) = registry.pending.get_mut(&self.ip) {
            *here -= 1;
            if *here == 0 {
                registry.pending.remove(&self.ip);
            }
        }
    }
}

/// One address's wrong keys: when each was sent, and how long the address
/// is turned away for, once it has sent too many.
#[derive(Default)]
pub(super) struct Strikes {
    at: Vec<Instant>,
    until: Option<Instant>,
}

impl Strikes {
    /// A wrong key sent at `now`. Strikes older than [`LOCKOUT`] are
    /// forgotten first: two typos this morning and one this afternoon are
    /// not somebody guessing.
    fn strike(&mut self, now: Instant) {
        self.forget(now);
        self.at.push(now);
        if self.at.len() >= WRONG_KEYS as usize {
            self.until = Some(now + LOCKOUT);
            self.at.clear();
        }
    }

    /// Turned away at `now`.
    fn locked(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| now < until)
    }

    /// Drop what has expired by `now`.
    fn forget(&mut self, now: Instant) {
        self.at.retain(|&at| now.duration_since(at) < LOCKOUT);
        if self.until.is_some_and(|until| now >= until) {
            self.until = None;
        }
    }

    /// Nothing left to remember.
    fn spent(&self) -> bool {
        self.at.is_empty() && self.until.is_none()
    }
}

pub(super) fn locked_out(shared: &Shared, ip: IpAddr) -> bool {
    let mut registry = lock(&shared.registry);
    let now = Instant::now();
    let Some(strikes) = registry.wrong_keys.get_mut(&ip) else {
        return false;
    };
    strikes.forget(now);
    let locked = strikes.locked(now);
    if strikes.spent() {
        registry.wrong_keys.remove(&ip);
    }
    locked
}

/// A wrong key costs the address that sent it, never the key: rotating the
/// key on a bad guess would let anyone on the LAN keep a host's card
/// changing just by guessing badly.
fn wrong_key(shared: &Shared, ip: IpAddr) {
    let mut registry = lock(&shared.registry);
    let now = Instant::now();
    // Every address that ever mistyped a key would otherwise stay in the
    // map for as long as the listener runs.
    registry.wrong_keys.retain(|_, strikes| {
        strikes.forget(now);
        !strikes.spent()
    });
    registry.wrong_keys.entry(ip).or_default().strike(now);
}

/// Take a registration, or say why not. Returns the bot's id and this
/// connection's serial.
pub(super) fn register_bot(
    shared: &Arc<Shared>,
    stream: &TcpStream,
    peer: SocketAddr,
    register: Register,
) -> Result<(BotId, u64), String> {
    if register.protocol != PROTOCOL {
        return Err(format!(
            "this listener speaks protocol {PROTOCOL}, not {}",
            register.protocol
        ));
    }
    let wire = Wire::of(stream).map_err(|e| e.to_string())?;
    // A reconnect: the token is who the bot is, and nothing else is asked.
    // A wrong one is no strike against the address: a token is too long
    // to guess, and a stale one is a bot that outlived its listener, not
    // somebody trying keys.
    if let Some(token) = &register.token {
        let token = Token::from_wire(token);
        let registry = lock(&shared.registry);
        let Some(id) = registry.bots.iter().position(|b| b.token == token) else {
            return Err("that token is not one this listener issued".to_string());
        };
        // Only the bot itself holds its token, so it may hear what became
        // of it.
        if registry.bots[id].retired {
            return Err(
                "this bot was let go here and its seat given away; register afresh".to_string(),
            );
        }
        return Ok(take_back(shared, registry, id, wire));
    }
    if register.name.is_empty() {
        return Err("a registration needs a `name`".to_string());
    }
    let mut config = lock(&shared.config);
    let mut registry = lock(&shared.registry);
    if let Some(id) = comeback(&registry, &config.admission, &register, peer.ip()) {
        drop(config);
        // A new token: the old one was lost with the bot that held it,
        // and it is not this registration's to be handed.
        registry.bots[id].token = Token::draw();
        return Ok(take_back(shared, registry, id, wire));
    }
    // The key, when one is asked for: found first and spent last, so a
    // registration refused for its name does not burn a single-use key.
    let invite = match &config.admission {
        Admission::Open => None,
        Admission::Keys(invites) => {
            let Some(given) = register.key.as_deref() else {
                return Err("this listener needs the key from its connection string".to_string());
            };
            let key = Key::parse(given);
            let found = key.as_ref().and_then(|key| {
                invites
                    .iter()
                    .position(|i| &i.key == key && i.uses != Some(0))
            });
            let Some(found) = found else {
                // A key this listener printed and has seen spent is a bot
                // started again with the string it was first started
                // with, its author's debug loop, not somebody trying keys:
                // counted, three restarts locked this machine out, the
                // fresh key on screen with it.
                let spent = key.is_some_and(|key| invites.iter().any(|i| i.key == key));
                drop(registry);
                drop(config);
                if spent {
                    return Err(
                        "that key has been spent: start the bot with the string shown now"
                            .to_string(),
                    );
                }
                wrong_key(shared, peer.ip());
                return Err("that key is not one this listener is taking".to_string());
            };
            Some(found)
        }
    };
    if config
        .max_bots
        .is_some_and(|max| registry.bots.len() >= max)
    {
        return Err("registration is closed".to_string());
    }
    if registry
        .bots
        .iter()
        .any(|b| !b.retired && b.info.name.eq_ignore_ascii_case(&register.name))
    {
        return Err(format!("the name {:?} is taken here", register.name));
    }
    let (bound_owner, slot) = match (&mut config.admission, invite) {
        (Admission::Keys(invites), Some(i)) => (invites[i].owner.clone(), invites[i].slot),
        _ => (None, None),
    };
    let owner_declared = bound_owner.is_none();
    let owner = bound_owner.or(register.owner);
    let key = owner_key(owner.as_deref(), owner_declared, peer.ip());
    // A name the host bound to a key of its own is that key's to use: on
    // the shared key anyone could claim it, and fill the owner's places
    // before the owner's own bot came.
    if owner_declared
        && let (Some(owner), Admission::Keys(invites)) = (&owner, &config.admission)
        && invites.iter().any(|i| {
            i.owner.as_deref().is_some_and(|o| {
                // The name as the invite binds it, against the name
                // as this bot would have it bound.
                owner_key(Some(o), false, peer.ip()) == owner_key(Some(owner), false, peer.ip())
            })
        })
    {
        return Err(format!(
            "{owner} has a key of their own here: register with that one"
        ));
    }
    // A bot that declares no owner is counted under its address, so leaving
    // the owner out is no way round the cap. The owners bots declared and
    // the ones the host bound have different keys (see `owner_key`), so a
    // bot that declared a name before the host invited somebody by it takes
    // none of that person's places, and the cup's draw tells them apart
    // the same way.
    if let Some(cap) = config.per_owner {
        let theirs = registry
            .bots
            .iter()
            .filter(|b| !b.retired && b.info.owner_key() == key)
            .count();
        if theirs >= cap {
            return Err(match &owner {
                Some(owner) => {
                    format!("{owner} already has {theirs} bot(s) entered, the most allowed")
                }
                None => format!(
                    "{} already has {theirs} bot(s) entered with no owner, the most allowed; \
                     declare an `owner`",
                    peer.ip()
                ),
            });
        }
    }
    if let (Admission::Keys(invites), Some(i)) = (&mut config.admission, invite)
        && let Some(uses) = &mut invites[i].uses
    {
        *uses -= 1;
    }
    let id = registry.bots.len();
    let token = Token::draw();
    registry.bots.push(Bot {
        info: BotInfo {
            id,
            name: register.name.clone(),
            version: register.version,
            owner,
            owner_declared,
            parallel: register.parallel.min(config.parallel_cap).max(1),
            slot,
            addr: peer.ip(),
        },
        token,
        key: invite.and_then(|i| match &config.admission {
            Admission::Keys(invites) if invites[i].slot.is_some() => Some(invites[i].key.clone()),
            Admission::Keys(_) | Admission::Open => None,
        }),
        conn: None,
        rtts: Vec::new(),
        pings: HashMap::new(),
        allowance: Allowance::new(),
        retired: false,
        absent: false,
    });
    drop(config);
    let serial = attach(&mut registry, id, wire);
    let msg = json!({
        "type": "registered", "name": register.name,
        "token": registry.bots[id].token.reveal(),
    });
    send(&mut registry, id, &msg);
    // A few pings straight away, so the console has a round trip to show.
    for _ in 0..3 {
        ping(&mut registry, id);
    }
    drop(registry);
    emit(shared, Event::Registered(id));
    Ok((id, serial))
}

/// The dropped bot a registration without a token is, if it is one: the
/// same name, not connected and not let go, and let in the same way, by
/// the same key (or, where none is asked for, from the same address).
///
/// A bot that restarts has lost its token with the rest of its memory,
/// and the console tells the host it is waiting for it to come back. As a
/// newcomer it was turned away: its name is still held for it, and its
/// single-use key was spent bringing it in the first time, which also
/// counted a strike against its address. The pair of key and name is
/// what it was let in on, so it is what takes its chair back, and only
/// while nobody is sitting in it.
///
/// Only a key printed for this one seat counts, and only while the host
/// still lists it: a cup's invite or an arena's `--key` is in several
/// entrants' hands and
/// bot names are public, so with it any rival could sit down in a
/// dropped bot's chair and throw its games. A bot let in on a shared key
/// comes back by its token or not at all.
fn comeback(
    registry: &Registry,
    admission: &Admission,
    register: &Register,
    ip: IpAddr,
) -> Option<BotId> {
    let key = register.key.as_deref().and_then(Key::parse);
    registry.bots.iter().position(|b| {
        !b.retired
            && b.conn.is_none()
            && b.info.name.eq_ignore_ascii_case(&register.name)
            && match admission {
                Admission::Open => b.info.addr == ip,
                Admission::Keys(invites) => {
                    b.key.is_some()
                        && b.key == key
                        && invites.iter().any(|i| Some(&i.key) == b.key.as_ref())
                }
            }
    })
}

/// Seat bot `id` on this connection, coming back by its token or by
/// [`comeback`]: told it is back, and every game it sits in told so.
fn take_back(
    shared: &Arc<Shared>,
    mut registry: MutexGuard<'_, Registry>,
    id: BotId,
    wire: Wire,
) -> (BotId, u64) {
    let serial = attach(&mut registry, id, wire);
    let msg = json!({
        "type": "registered", "name": registry.bots[id].info.name,
        "token": registry.bots[id].token.reveal(), "resumed": true,
    });
    send(&mut registry, id, &msg);
    let routes: Vec<(Sender<GameMsg>, PlayerId)> = routes_of(&registry, id);
    drop(registry);
    for (tx, seat) in routes {
        let _ = tx.send(GameMsg::Back { seat, serial });
    }
    emit(shared, Event::Reconnected(id));
    (id, serial)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wrong keys spread out over more than the lockout window never add
    /// up to one: each strike is forgotten once the window has passed.
    #[test]
    fn wrong_keys_far_apart_never_lock_an_address_out() {
        let start = Instant::now();
        let mut strikes = Strikes::default();
        for i in 0..10 {
            let now = start + LOCKOUT * i + Duration::from_secs(1);
            strikes.strike(now);
            assert!(!strikes.locked(now), "strike {i}");
        }
        // Three close together still do, for the lockout and no longer.
        let now = start + LOCKOUT * 20;
        for s in 0..3 {
            strikes.strike(now + Duration::from_secs(s));
        }
        assert!(strikes.locked(now + Duration::from_secs(3)));
        let later = now + Duration::from_secs(2) + LOCKOUT;
        strikes.forget(later);
        assert!(!strikes.locked(later));
        assert!(strikes.spent());
    }
}
