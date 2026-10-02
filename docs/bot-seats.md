# Bot seats: design

**Status:** built, 2026-10-02: phases 1 to 6 (seat controllers, the
protocol and the arena, cups, the fair cursor, Join as bot, and straight
to the host). Phase 7 (watching a cup live) is left for later, as the
design says. The bot author's contract is `bot-protocol.md`. Where the
build departs from this design it says so at the end, under *As built*.

## Goal

People should be able to write their own bots, in any language, register
them, and enter them in cups against each other, against the game's
AI, and against humans, on the same beach and under the same rules. The
main reason is competitions between bots people wrote. The second is what
those matches teach about the game: a strong bot exploiting a rule is the
fastest way to find a weak rule.

So there is one kind of match, and a seat in it can be held by any of four
things:

| Seat | Who decides | Where it runs |
|---|---|---|
| Local human | keyboard or pad, through a cursor | this machine |
| Remote human | a person on another peer | their machine, inputs over the wire |
| Game AI | `bot_action`, a pure function of the board | every peer, identically |
| Bot | a program someone wrote | its author's machine, connected over TCP |

LLM agents are not a fifth kind. They are a bot that answers slowly, and
they get a clock that suits them (see *Clocks*).

## The one rule: the game never runs a bot

A bot is a network client. It runs wherever its author runs it, on a
laptop at the party or a server at home, and **connects** to the game, the
way a player joins a lobby. The game only ever listens, sends the board,
and reads moves back.

This is what makes a cup something anyone can enter: an organiser
never has to install anyone's language, trust anyone's code, or sandbox
anything, because nothing of the entrants' runs on the organiser's
machine. It also means the bot an author tests at home is byte for byte
the bot that plays in the final.

## What already exists

The sim is already indifferent to who sits where. `Board::tick` takes one
`PlayerAction` per seat and nothing else, and the shell already fills those
six slots from three sources: the cursors, the AI fill
(`fill_bot_actions`, `session/tick.rs:13`), and the lockstep wire.
`examples/balance.rs` plays thousands of headless rounds through the same
call. Replays store the level and the per-tick actions, and `PINCH_REPLAY`
plays one back in the real game.

Nothing in this design changes the sim, its tick order or its state hash.
Everything is shell-side. The one format that grows is the replay's, by a
line saying what kind each seat was (see *Who is who*).

## The seat controller

Name what the shell already does. A **seat controller** is anything that
produces one `PlayerAction` for its seat each tick. Today the kinds are
spread across `Bots`, the cursor systems and `OnlineSession`; the first
step is to gather them behind one per-seat description:

```rust
enum SeatController {
    /// A local human: which keys or pad drive the cursor.
    Local(SeatInput),
    /// A seat owned by another peer; its inputs arrive over the wire and
    /// this machine never decides for it.
    Remote,
    /// The game's AI at a difficulty.
    Ai(BotLevel),
    /// A bot connected to this machine over the bot protocol.
    Bot(BotConnection),
}
```

`MatchConfig` then carries a controller per seat instead of a `seats`
count, a `bots` count and a `bot_levels` array, and match setup offers the
kind of each seat as a dial.

### The game AI and bots are different on the wire

This is the one distinction the design has to keep, and it comes from
lockstep. The game AI is free online because every peer computes it from
the same board through the same code, so its moves never cross the wire
and cannot diverge. A bot cannot work that way: it talks to one machine,
may be nondeterministic, and nobody else can reproduce it.

So **a bot's seat is owned by the one game it connected to, like a
human's**, and its actions travel over the lockstep wire like a keyboard's.
The other peers see a `Remote` seat and cannot tell the difference, and do
not need to. The game AI stays computed everywhere.

## Where a bot can sit

- **The arena and cups.** Headless matches run by
  `pinch-points arena` and `pinch-points cup`, where every seat is a
  bot or the game AI. This is the main case (see *Cups*).
- **Couch play.** Match setup can mark a seat as open to a bot. The screen
  shows a connection string for it, the same card as "Join as bot", and
  the bot that registers with that string takes the seat. Two friends and
  two bots on one screen. The card listens on localhost, for a bot on the
  same machine; "open to the LAN" turns the string into a single-use one
  on the LAN address, under the same rules as route 2, for a friend's bot
  on their own laptop.
- **A party in the lobby.** A bot joins a LAN party the humans are in, as a
  player, tagged as a bot. Two routes, below in *Joining a party*.

## Joining a party

A bot can take a seat at an ordinary LAN party, beside the humans, under
the humans' rules. There are two routes in, and they speak the same bot
protocol, so a bot cannot tell which one it is on and an author writes it
once.

### Route 1: through its author's game ("Join as bot")

The author opens the game on their own laptop and picks the party from the
lobby list like anyone else, but chooses **Join as bot** rather than
**Join**. Their game listens on localhost, their bot connects to it there,
and the game joins the party as an ordinary peer whose one seat is driven
by the bot instead of the keyboard.

- **Almost nothing new on the wire.** A peer already holds exactly one
  seat (`NetMsg::Start` deals one `seat`), and its inputs already travel
  like anyone's. What is added is a flag saying the seat is a bot, on
  `Hello`, `Roster` and `Start`.
- **The author watches from their own screen**, which is the best
  debugging seat in the house: their bot's view of the match, drawn, with
  its `note`s in the terminal.
- **It is how this game is played.** A friend comes round with a laptop
  and their bot on it.
- **The party sees whose bot it is.** The seat is named for the bot and
  its owner, "Greedy (Ana's bot)", from the bot's registered `name` and
  the player name the author's game already sends in `Hello`. In a lobby
  full of kids, a bot always has a person behind it that everyone can
  see. Two seats with one name are the party roster's business, handled
  the way it already handles two humans with one name; a bot's name is
  only unique on the listener it registered with.

The screen it opens is a bootstrap card: the one line the bot needs, a
way to copy it, and what has connected so far.

```
  Join Ana's beach as a bot

  Start your bot with:

    pinch://127.0.0.1:47710/7F3K-9QXA        [C] copy

  Waiting for your bot...
```

and once a bot has registered with that key:

```
  Connected: Greedy 0.3   round trip 0.1 ms
  Joining Ana's beach...
```

The line is a connection string (see *Connection string*), and the author
pastes it as the bot's argument:
`python3 greedy.py pinch://127.0.0.1:47710/7F3K-9QXA`. Copying goes
through the same clipboard path as the share codes, which already fails
politely where there is no clipboard (`code_copy_failed`). The card shows
the bot's own `name` and `version` from its `register`, so the author can
tell it is the build they meant to start and not last week's. The game
joins the party only once a bot has registered, so a party never sees a
seat that nothing is driving; backing out of the card gives the key up.

### Route 2: straight to the host

The hosting card shows a connection string,
`pinch://192.168.1.20:47710/M2QD-7WTR`, and a bot anywhere on the network
connects with it, with no game window on its side at all, which suits a
bot that lives on a home server. Here the listener is on the LAN rather
than localhost, so the key is what keeps the chair for the bot the host
invited: it is single-use, spent the moment a bot registers with it, and
the card then draws a fresh one for the next. A bot that drops comes back
with the secret token it was given at registration, never with the key,
so a key read off the screen is worth nothing once it is spent. The seat
reads with the owner the bot declares, "Greedy (Ana's bot)", or "Greedy
(bot)" if it gives none; the host has no way to check it, which the tag
section says out loud. A string on the host's screen is an invitation
that the people in the room can read, which is the right strength for a
party and no more. The host then owns that seat and forwards its actions
to the table. Today the host
sends inputs for its own seat only; `InputMsg` already names its
`player`, so the wire can say it, but the session has to learn that the
host speaks for several seats. That is shell work and a
`PROTOCOL_VERSION` bump, not a change to the sim, and it is the more
expensive route, so it comes second.

### The bot tag

- **A drawn robot icon, not an emoji.** The game ships DejaVu Sans Mono
  and a Japanese subset of Noto Sans Mono CJK, and neither has 🤖, so the
  emoji would draw as an empty box. The icon is a sprite like the rest of
  the art (`tools/gen_sprites.py`), in the seat's flag colour, beside the
  name wherever a name appears: the lobby roster, the party list, the HUD,
  the results card, replays and cup standings.
- **The game sets it, never the name.** A bot that registers as "Ana"
  still wears the robot. In a game played by kids, nobody should mistake a
  bot for a friend.
- **What it can promise.** On route 2 the host sets the tag itself, and
  cannot be lied to about it, but the owner in brackets is only what the
  bot declares. On route 1 it is the other way round: the owner is the
  player name the author's game sends, while the tag comes from that game,
  so a modified build could leave it off. Among friends on a LAN those are
  acceptable limits; they are written down so nobody mistakes them for
  guarantees.

### The host stays in charge

- **"Bots welcome".** A switch on the hosting card, carried in the beacon,
  so the party list says whether a beach takes bots before anyone walks
  up to it. The beacon grows only at its tail by design, so the field is
  added without breaking older builds, which read it as absent. Proposed
  default: on, since the party is where people first meet the feature.
- **Kicking.** There is no kick in the lobby today, for anyone. A bot that
  plays badly or chats too much makes the gap obvious, and the host needs
  it for humans as much as for bots. It is built with this, not after.
- **The fair cursor is on** in any lobby with a human in it, and is not a
  dial there. A party where the bot places anywhere instantly is not a
  party the humans enjoy.

### What already works

- **A peer that leaves mid-round** hands its chair to the game AI through
  the existing `Abandoned` handover, exactly as a human who walks out does.
  That covers the author's game quitting on route 1 and the host losing a
  route-2 bot for good.
- **A bot that drops but whose game stays** is the case `Abandoned` does
  not see: on route 1 the author's game is still connected and still
  sending inputs, empty ones, so the host never hands the seat over and
  the party would look at an idle chair. So the author's game stands in
  itself. After a grace period (5 s) it runs `bot_action` for the seat
  and sends those moves as its own inputs, which to the table are just
  inputs: no host decision, no frame to agree on, no desync. The seat
  shows the game AI standing in, and a bot that reconnects with its token
  takes it back on the next tick. The host does the same for a route-2
  bot.
- **Series.** The bot stays in the lobby between rounds like a player, and
  gets a fresh `hello` for each round.
- **The clock** is the live one: the reply has to land before the seat's
  next commit, 33 ms, which on a LAN is plenty. It is the clock the humans
  are on.

## Clocks

Bots are meant to be competitive and fast, so the clock is strict and the
same everywhere.

**One deadline, everywhere.** Every tick a bot is sent has the same
deadline: one tick, 33 ms, from the moment it was sent. A reply that lands
in time is that tick's action; a bot that misses it does nothing that tick.
That is the rule at a party, in the arena and in a cup, so a bot tuned in
one behaves the same in the others, and a bot only ever has to count
ticks, never wall time.

**What differs is only when the next tick is sent.**

- **Live** (any match with a human in it). On the wall clock, 30 times a
  second, because humans are playing. The sim never waits for a bot, so a
  slow bot costs only itself and never stalls a table. Online this is
  exactly a keyboard's position: the commit is scheduled `DEFAULT_DELAY`
  frames ahead, like any local input.
- **Fast-forward** (all-bot matches: the arena and cups). The next tick is
  sent as soon as every bot has answered this one, or its deadline has
  passed, whichever is first. Idle time is skipped, never handed out: no
  bot ever gets more than its deadline, and a round whose bots answer in a
  millisecond takes a couple of seconds instead of three minutes. A bot
  that said `wait` is not waited for on the ticks it skipped. This is what
  makes a cup of hundreds of games fit in an evening.

**Which tick a reply acts on.** A reply to tick `t` takes effect on the
transition `input_delay` ticks later, and `hello` says what that is. In
the arena and a cup it is 0: the reply to `t` is the action the sim takes
from `t` to `t + 1`. On a couch it is 0 too. Online it is `DEFAULT_DELAY`
(3), because a seat's input is committed that far ahead for the whole
table, a human's and a bot's alike. So the deadline is the same
everywhere but the aim is not: a bot that places in front of a moving crab
has to lead it by the delay, exactly as a person online does, and a bot
that ignores `input_delay` will play well in the arena and miss at a party.

**Thinking is what happens before the reply.** The deadline guarantees a
bot the time until it answers, and nothing after. Live, a bot that
answers at once and keeps computing in the background still gets the
rest of the 33 ms of wall time, because the next tick waits for the wall
clock. In fast-forward it gets only as long as the slowest bot takes to
answer, which may be nothing. The spec says so plainly: a bot that wants
the whole tick should think first and answer at the end of it.

The flip side is that a cup's length is set by its entrants. A bot that
always uses its full 33 ms turns every game it plays into real time, three
minutes a round, so "a cup in an evening" holds for bots that answer
quickly and not otherwise. The standings print each bot's reply times,
which makes it visible who is slow, and `parallel` is how a slow bot
keeps a cup moving anyway.

**The live deadline is nominal.** At a party the ticks come from the
game's fixed-rate update, and when a frame runs long that update runs
several ticks back to back to catch up, so two ticks can go out almost
together. The listener reads bot replies on its own thread, never the
frame's, and each commit takes the newest reply in hand at that moment.
A bot loses such a tick the way it loses any late one, and the log says
so.

**Late replies.** A late reply still counts at the next tick, unless a
newer reply has arrived by then, and the log records it as late. Lateness
never buys time: each tick's deadline starts when that tick is sent, so a
bot that is always late is always a tick behind, not a tick richer.

**When to raise the deadline.** The deadline is measured at the listener,
so it includes the network. One tick is right wherever the round trip is
small against it:

| Venue | Round trip | Deadline |
|---|---|---|
| Same machine | under 0.1 ms | one tick (the default) |
| LAN | about 1 ms | one tick (the default) |
| Internet | 20 to 150 ms | raised, `--deadline 200` or more |
| LLM agents | seconds | a slow match, `--deadline 10000`, with `wait` |

A raised deadline is one number for the whole match, the same for every
seat, and printed in the header and the standings, since results under
different deadlines are not comparable. A slow match needs its slow bots
to use `wait`: 5400 ticks at ten seconds each is fifteen hours, while an
LLM that decides once a second of game time (`"wait": 29`) makes 180
decisions, half an hour. A raised deadline only exists in fast-forward:
a live match runs on the humans' clock, and a bot there has one tick. A
slow match is labelled as one wherever its results appear.

The listener measures each bot's round trip with pings outside the
deadline and prints it in the standings, so a bot that keeps losing on
distance shows it.

Either way the replay records the actions actually committed, so a replay
reproduces the match exactly. *Re-running* a match reproduces it only if
the bots are deterministic and met every deadline, which is a property of
the bots, not of the listener.

## The protocol

A public contract: once people write bots against it, changing a field
breaks them. It carries a version from the first message, as the snapshot
and replay formats do, and a bot that asks for a version the listener
does not speak is refused with a clear message rather than half-understood.

### Transport

TCP, one JSON object per line, UTF-8, in both directions. A bot in any
language is a socket and a JSON library, and the protocol can be tried by
hand with `nc`. `serde_json` is already a dependency (the update check),
and TCP is in the standard library, so the listener needs no new crate.

The default port is **47710**, beside the lobby's UDP range
(`LOBBY_PORTS`, 47700 to 47707), and every command takes `--port`.

Plain TCP is right for a LAN. On the internet the token (see
*Registration*) must not travel in the clear, so an internet server sits
behind a TLS-terminating proxy (Caddy, stunnel, nginx) and bots connect to
that. The game speaks plain TCP either way and needs no TLS dependency of
its own.

Coordinates are tiles, `x` to the right and `y` down, `(0, 0)` top left,
matching the level format and `Direction::delta`. Directions are
`"up"`, `"down"`, `"left"`, `"right"`.

### Hardening

Every listener treats every byte it receives as hostile: the arena's, the
"Join as bot" card's on localhost, the hosting card's on the party's LAN,
and a cup server's. A bot with a bug is as dangerous to a listener as a
bot with a grudge, and the route-2 listener is on the machine drawing a
party's game.

- **Bounded input.** A line longer than 64 KiB closes the connection. A
  bot that floods is rate-limited, then dropped. A message is parsed into
  a fixed shape and nothing else, and no field ever becomes a path, a
  command or a format string.
- **Bounded work.** `simulate` is capped per decision, and `parallel` per
  entrant (8 by default), so no one bot can make a listener do the work of
  many.
- **Wrong keys cost the sender, not the key.** Three wrong keys from one
  address and that address is refused for a minute. The key itself never
  rotates on a wrong guess, or anyone on the LAN could keep a host's card
  changing just by guessing badly.
- **Tokens are secrets.** Random, never printed, never logged, never in
  standings. Over the internet they travel only inside TLS, from the
  proxy described under *Transport*.
- **Names are display text.** Length-capped and drawn through the same
  text path as seat names, which already copes with any script.

### Performance

A competitive bot spends its tick thinking, not waiting on the pipe, so
the protocol keeps its own overhead small and measured.

- **`TCP_NODELAY` on both ends.** Nagle's algorithm meeting delayed ACKs
  can hold a small message for up to 40 ms, longer than the whole
  deadline. Every listener sets it, the spec tells bot authors to, and
  every example bot does, in its first lines.
- **Full state every tick, no deltas.** A busy board (an XL beach at its
  crab cap, `CRAB_CAP_TILES_PER_CRAB`) is about 10 KB a tick, twice that
  while Crab Mania floods past the cap: some 300 to 600 KB a second per
  bot, which is nothing on a LAN and a fraction of a millisecond to parse
  in any language. In exchange a bot never has to reconcile a missed
  update, and a stateless bot is a correct one.
- **Measured at the listener.** Every reply's time from tick sent to reply
  read is logged, and each bot's median and 99th percentile go in the
  standings. Those two numbers are what an author optimises, and the 99th
  is the one that costs ticks.
- **The sim is portable on purpose.** `simulate` is fair, but it costs a
  round trip out of the deadline, so a serious bot runs its own sim. A Rust
  bot links the crate. For everyone else the rules document is precise
  enough to port from (integer subunits, the frozen tick order of spec
  §4.4), and `arena --trace` writes every tick message with the tick after
  it, so a port can be checked against the real thing line by line.
- **Parallel games.** A bot that can play several games at once
  (`"parallel"` in `register`) finishes a cup sooner; a cup is bounded by
  the slowest entrant's capacity.

JSON is the encoding for version 1: readable, in every language's
standard library, and testable with `nc`. If profiles ever show parsing
as the cost, a compact encoding of the same fields can be negotiated in
`register`, without touching the messages' meaning.

### Connection string

Every way in hands the bot the same thing, one line:

```
pinch://HOST:PORT/KEY
```

The arena prints one, the "Join as bot" card and the hosting card show
one, and a cup invite is one. So a bot's command line is always
`mybot <string>` whatever it is joining, and an author never has to learn
a second way to start their bot. The protocol spec ships a parser for it
in each example bot.

- `KEY` is eight base32 characters in two groups (`7F3K-9QXA`), which is
  40 bits and short enough to read aloud across a room. It is optional:
  an open cup with no invite prints `pinch://HOST:PORT` alone.
- `pinchs://` in place of `pinch://` means "connect with TLS", for a
  server on the internet behind the proxy described under *Transport*.
  The game itself only ever prints `pinch://`; the organiser writes the
  `pinchs://` form for the proxy's address.

The key in a string and the token a registration returns are two
different things, and the protocol never uses one word for both. The key
gets a bot in; the token is who it is afterwards.

| | Join key | Token |
|---|---|---|
| Issued by | the screen or command that printed the string | the listener, on every registration |
| Says | "this is the connection I invited" | "this is the bot that registered" |
| Lives | until it is spent, or its card or run closes | the listener's life: a card's session, an arena run, a whole cup |
| Used | once, to register | on every reconnect |
| Seen by | whoever can see the screen | the bot alone; never printed, never logged |

The rules for a join key:

- **Drawn fresh** for every card, arena run and invite. Nothing is reused
  from a previous session.
- **It admits, it does not identify.** A key is spent by the registration
  it lets in, and a bot that drops comes back with its token. A key that
  anyone could read off a screen can therefore not be used to take over a
  seat once its bot has it.
- **A cup invite is the one shared key.** One key for every entrant,
  spent by nobody, and good only for registering: after that each bot is
  its own token.

What neither can prove is **which code is running**. Nothing on the wire
can show that the process holding a key is the bot its author says it is.
The key proves the connection was handed the string, the token proves it
is the entrant that registered earlier, and that is as far as it goes.

### Registration

The first thing a bot sends, on every connection:

```json
{"type": "register", "protocol": 1, "name": "Alice's crab herder",
 "version": "0.3", "owner": "Alice", "parallel": 4}
```

- `name` is how the bot appears in standings and on screen. Names are
  unique per listener; a taken one is refused.
- `parallel` is how many games the bot will play at once over this
  connection. A cup runs many games in parallel, and a bot that can
  keep up with several finishes the cup sooner. Default 1. (Not `games`:
  `arena --games N` already means how many games to play in a row.)
- `owner` is the person behind the bot, shown beside its name and used by
  a cup to keep one author's bots apart (see *Who may enter*). Where the
  listener already knows the owner, from the author's own game on route 1
  or a per-author invite in a cup, it uses that and ignores this field.
- `key` is the join key from the connection string, when the string has
  one. A wrong or missing key where one is required is refused with a
  message saying which.

The listener answers with a token, whatever it is: an arena, a "Join as
bot" card, a hosting card or a cup server.

```json
{"type": "registered", "name": "Alice's crab herder", "token": "k3F9...Qz"}
```

The token is the bot's identity for the life of the listener. A bot that
reconnects sends `{"type": "register", "token": "k3F9...Qz", ...}` and is
the same bot, seat and standings and all; a bot that connects without it
is a new registration, which needs a key where the listener asks for one.
Nobody can play under a name they did not register.

### Games are multiplexed

Every message after registration carries the `game` it belongs to, so one
connection plays several games at once. A bot that says `"parallel": 1` sees
one `game` at a time and can ignore the field.

### Handshake

When a game starts, the listener sends the static half of the board and the
rules:

```json
{"type": "hello", "game": 17, "seat": 2, "seats": 4,
 "names": ["Ana", "Normal AI", "Alice's crab herder", "bob"],
 "kinds": ["human", "ai", "bot", "bot"],
 "board": {"width": 12, "height": 9, "wrap": false,
           "tiles": ["............", "..R.....K...", "..."],
           "walls": {"h": ["..."], "v": ["..."]},
           "spawners": [{"x": 0, "y": 4, "dir": "right", "period": 45}]},
 "rules": {"signpost_cap": 3, "cap_policy": "evict",
           "signpost_lifetime": 300, "round_ticks": 5400,
           "gull_period": 300, "castle_raids": true, "events": true},
 "clock": {"mode": "live", "deadline_ms": 33, "input_delay": 3},
 "cursor": {"fair": true, "ticks_per_tile": 3}}
```

The bot answers `{"type": "ready", "game": 17}` before the first tick. A bot
that does not answer within ten seconds forfeits the game.

### Each tick

Everything a player can see on the screen, and nothing they cannot:

```json
{"type": "tick", "game": 17, "tick": 1234, "remaining": 4166,
 "scores": [12, 3, 20, 7],
 "you": {"signposts": 2, "last": {"accepted": false, "reason": "rival_post"}},
 "cursors": [[3, 4], null, [5, 5], [10, 7]],
 "crabs": [{"id": 88, "x": 5, "y": 3, "dir": "left", "progress": 96,
            "kind": "giant", "claw": "right"}],
 "gulls": [{"id": 4, "x": 9, "y": 2, "dir": "down", "progress": 0,
            "state": "flying", "hops_left": 2, "claw": "left"}],
 "signposts": [{"x": 5, "y": 2, "dir": "up", "owner": 2, "worn": false,
                "age": 41}],
 "castles": [{"x": 1, "y": 1, "owner": 0}],
 "turnstiles": [{"x": 6, "y": 4, "next": "right"}],
 "event": {"name": "gull_mania", "ticks_left": 120},
 "lure": null, "claw_call": false, "surge": false}
```

`remaining` is `null` on an untimed beach. `surge` is the closing stretch
the HUD already draws (`Board::in_surge`), when the gull rate doubles.

A cursor is `null` where the listener cannot see it. The lockstep wire
carries actions and never cursor positions (`InputMsg`), so a game knows
the cursors of its own seats and nobody else's: on route 1 the remote
humans' cursors are `null`, while in the arena, where every seat is the
server's, none are.

Castles and turnstiles are repeated every tick because they change during
play (`CastleSwap` trades owners, a turnstile flips on every crossing).
`progress` is in subunits of 256 per tile, the sim's own unit, so a bot
can compute arrival times exactly; `CrabKind::speed`, the gull speeds, and
the caps that pause the spawners (`CRAB_CAP_TILES_PER_CRAB`, `GULL_CAP`) go
into the published rules document with it.

### The reply

At most one action per tick, as the sim takes, each naming the tick it
answers:

```json
{"game": 17, "tick": 1234, "act": "place", "x": 5, "y": 2, "dir": "left"}
{"game": 17, "tick": 1234, "act": "remove", "x": 5, "y": 2}
{"game": 17, "tick": 1234, "act": "clear"}
{"game": 17, "tick": 1234, "act": "move", "x": 5, "y": 2}
{"game": 17, "tick": 1234, "act": "none"}
```

`move` puts the seat's cursor on a tile without doing anything there. It
is a no-op for the sim, and it only means something under the fair
cursor, where parking the cursor near where the next placement will be
is a skill a human uses all the time.

`clear` is the human's clear-all key: it removes the seat's first
signpost wherever it stands (`Board::first_signpost_of`), one per tick, as
`play_input.rs:204` does for a held key. It matters under the fair
cursor, where it is the one removal that needs no walk, as it is for a
person.

The `tick` is what lets a listener tell a fresh reply from a stale one:
the newest reply for a seat wins, and one answering an older tick than a
reply already in hand is dropped. Without it, a late reply, two replies in
one window, and a reply after a `wait` would all be guesses.

Two optional fields on any reply:

- `"wait": k` says "do not ask me again for `k` ticks". Those ticks are
  not sent at all, so a bot that thinks in half seconds is not woken
  thirty times a second and the listener sends nothing it will not read.
  Nothing is lost: the next tick it gets carries the full state. In the
  arena it is what lets an all-bot round run at full speed. Under the fair
  cursor a walk in progress carries on through the wait.
- `"note": "..."` is written to the decision log beside the action. An LLM
  bot puts its reasoning here, and it is the raw material for learning
  from a match. Notes are capped at 4 KiB and cut, not refused, so a long
  thought never costs a bot its connection under the 64 KiB line limit.

A reply that is not valid JSON, or names an action that does not exist,
counts as `none` and is logged.

A placement the sim rejects (a rock, a castle, a rival's post) is reported
back in the next tick's `you.last`, with a reason. Those reasons are new
work: `Board::can_place_signpost` answers only yes or no, so it grows a
reason type that the yes-or-no becomes a view of.

`PlayerAction::CallEvent` is the spectators' action, relayed by the host,
and is not available to bots.

### Disconnects

A dropped connection is normal on a real network, and it costs the bot,
never the match. Its seats idle from the tick the connection went quiet
(at a party the game AI stands in after a grace period; see *What already
works*).
If it reconnects with its token while a game is still running, the
listener sends that game's `hello` again, marked `"resumed": true`, and the next
tick, and the bot plays on. Games that had not started yet wait for it
until the organiser's forfeit timeout (default 60 s), then are played with
the seat idle and scored as a forfeit: last place, whatever the idle seat
happened to bank from crabs wandering into its castle, with the other
seats placed among themselves.

### Looking ahead

A Rust bot can link the crate and simulate the board forward itself. That
is a large edge (crab movement is deterministic, only the rivals and the
spawns are not), and a protocol that leaves it to Rust quietly favours
Rust. So the listener answers a lookahead for everyone:

```json
{"type": "simulate", "game": 17, "ticks": 60,
 "plan": [{"at": 0, "act": "place", "x": 5, "y": 2, "dir": "left"}]}
```

It runs a copy of the board with the given plan, every rival idle, and
answers with the predicted tick message at the end, plus a report of what
happened on the way: every crab banked (which crab, which castle, which
tick), every crab eaten, every signpost worn or washed away. That is what a
bot actually wants from a lookahead, and it is a few hundred bytes where
600 intermediate states would be six megabytes. It counts against the bot's
own deadline like any other thinking, and the listener caps it (600
simulated ticks per decision by default), since a lookahead runs on the
listener's machine and a bot that asks for a million ticks is asking it to
stall. Under the live clock the cap is 60 ticks and the lookahead runs off
the frame's thread: on route 2 the listener is the host, drawing a party's
game, and a bot's thinking must never be a hitch the humans feel.

The lookahead must not leak what a human cannot know, and the board holds
two such things: the PRNG, which decides where the next gull lands and
what the roulette spins, and each gull's `takeoff_in` countdown, which the
screen never shows. So the copy is reseeded from a fixed seed, and what
the PRNG decides comes from that draw instead. Where the schedule is
public, it is kept:

- **Spawner crabs keep their schedule.** A spawner emits on
  `tick % period` (`crabs.rs:50`), and the period is in `hello`, so when
  a crab appears is something a player can count. What it is, its kind
  and its claw, comes from the reseeded draw, and it is marked
  `"predicted": true`.
- **Ambient gulls are left out.** They arrive on `gull_period`, which is
  public, but land on an edge tile the PRNG picks, and a gull on a guessed
  tile is worse than none.
- **Gull takeoffs** come from the reseeded draw.

The prediction is exact about everything already on the sand, honest
about when the next crabs come, and silent about the rest, which is what a
player reading the board knows.

The Rust starter kit gets the same rule for free. It builds its board
from the tick message through a `Board::from_observation` helper, and the
observation carries no PRNG state, so a Rust bot's own lookahead knows
exactly what the `simulate` request knows.

### End of a game

```json
{"type": "end", "game": 17, "scores": [12, 3, 20, 7], "placing": 1,
 "replay": "t1-g17"}
```

The bot, or its author with a small script, fetches any game it played
in with `{"type": "replay", "id": "t1-g17"}` and gets the replay text back.
An author who was not at the venue can still watch every one of their
bot's games.

## The fair cursor

The biggest gap in a mixed table. A human moves a cursor to a tile before
placing: the default hold-to-repeat is 0.28 s to the first repeat, then
0.09 s a tile (`settings/mod.rs:365`). The sim has no cursor at all, so the
game AI, and any bot, places anywhere instantly. That is already true of
the AI today, and a strategy that places in opposite corners on
consecutive ticks is one no human can copy, so lessons from bot matches
would not transfer to human play.

The **fair cursor** is a match rule that gives every non-human seat a
cursor moving at human speed. The bot's reply names a target; the listener
walks the seat's virtual cursor there at `ticks_per_tile` (3, near the
0.09 s repeat), and commits the action on arrival. The replies mean:

- **`place` or `remove`** sets a new order: walk to that tile, then act.
  It replaces any order still walking.
- **`move`** is an order with nothing at the end of it: walk there and
  stop.
- **`none`** means "no new order". A walk in progress carries on; `none`
  never cancels one, so a bot does not have to repeat its order every
  tick.
- **`clear`** needs no walk, as the clear-all key needs none for a person.

The cursor lives in the seat layer, outside the sim, so replays and the
state hash are untouched: a replay records the committed action, as it does
for a human.

The seat's own cursor goes into the tick message, and so does every other
cursor the listener can see (see *Each tick*); at a couch, humans see each
other's cursors, online they do not.

**One rule for every seat at the table.** The rule covers every non-human
seat, the game AI included: a table where the bots walk and the AI teleports
would make bots slower than the AI beside them. So:

- **Any match with a human in it:** on, for bots and the game AI alike,
  and not a dial. This is what lessons from bot play need to transfer to
  people.
- **All-bot matches** (the arena, cups): off by default, since every seat
  is equally instant and no human is there to be fair to. A flag turns it
  on, for authors who want their bot to learn human-shaped play.

Putting the game AI under it changes the AI's strength, so the phase that
builds the cursor also re-measures the balance budgets in
`examples/balance.rs` and the difficulty ladder in `examples/ladder.rs`.
Until that phase lands, the cursor does not exist and nothing claims it.

## Who is who

A seat's kind is shown wherever its name is: the lobby roster, the HUD,
the results card and the replay. Nobody should be surprised by what they
are playing against, and a results screen that marks bots is how a
cup's standings read at a glance. For bots that mark is the robot
icon (see *The bot tag*). On the wire this is a kind byte per seat on
`Hello`, `Roster` and `Start`, one `PROTOCOL_VERSION` bump that route 1
needs and route 2 shares.

Replays carry it too, or `watch` could not draw it: the replay stores the
seat names (`names:`, `replay.rs:42`) and nothing about kinds. A `kinds:`
line beside `names:` holds them. Whether that is a `replay-v3` header or a
line v2's reader already tolerates is decided when it is built, by the
rule the replay format already follows: a recording that would be
misread is refused, never half-read.

## The bot author's loop

Writing a bot is a loop of four steps: edit, play, read the log, watch.
Every step needs a command, or the loop has a hole in it. All of them are
subcommands of the shipped binary, so an author needs a release download
and their own language, and no Rust toolchain.

**Play** against the game's AI. The arena opens the given seats and waits
for bots to connect to the rest:

```
$ pinch-points arena --seat ai:easy --open 1
Waiting for 1 bot. Start it with:

  pinch://127.0.0.1:47710/H4TN-C2LV
```

In another terminal, the author starts their bot with that string:

```
$ python3 greedy.py pinch://127.0.0.1:47710/H4TN-C2LV
```

```
  Greedy connected (round trip 0.1 ms)
Beach: classic, seed 7731, standard round (3:00), fair cursor off
Playing... done in 1.8 s (5400 ticks)

  #  Seat  Player          Kind   Score  Banked  Raided  Rejected  Late
  1  P2    Easy AI         ai        41      38       1         0     0
  2  P1    Greedy          bot       17      15       3        22     4

Replay:  ~/.local/share/pinch-points/arena/2026-09-29-1402.replay
Log:     ~/.local/share/pinch-points/arena/2026-09-29-1402.log
```

`--open 2` prints one string per open seat, and starting the same bot
twice, once with each, is how an author plays their bot against itself,
or against last week's version. `--key KEY` fixes the key instead of
drawing one, so a script that runs the arena and the bot together knows
the string in advance.

| Flag | Default | What for |
|---|---|---|
| `--seat ai:<level>` | | a seat for the game's AI; repeatable |
| `--open N` | 0 | seats left for bots to connect to |
| `--listen ADDR` | `127.0.0.1:47710` | where to wait; `0.0.0.0:47710` to let a friend's bot in |
| `--key KEY` | drawn fresh | a fixed join key, for scripts that start the arena and the bot together |
| `--map classic\|generated\|FILE` | `classic` | the beach |
| `--seats N` | seats given | table size on a generated beach |
| `--seed S` | random, printed | the same beach again after a change, for a fair before and after |
| `--round short\|standard\|long` | `standard` | round length |
| `--games N` | 1 | play N seeds and print averages, so a bot is not judged on one lucky round; the bots stay connected between games |
| `--deadline MS` | 33 (one tick) | raise it only to match a cup that raised it, or for a slow match |
| `--trace FILE` | off | write every tick message and the tick after it, to check a ported sim against |
| `--fair-cursor on\|off` | `off` | the cursor rule (from phase 4) |
| `--watch` | off | draw the match in a window as it plays, at real speed |

The arena listens on localhost unless told otherwise, so a practice arena
is never open to the network by accident.

Without `--watch`, no window is opened and nothing from Bevy is started:
it is the sim, the seat controllers and the protocol, and it runs as fast
as the bots answer. `--watch` is the most natural way to debug a bot: the
author sees the wrong placement as it happens, with the bot's `note` for
it in the terminal beside the window. It plays in real time, since a match
drawn faster than a human can follow shows nothing.

**Read the log.** It holds every decision each bot made, with the tick,
the action, its `note`, and whether it was rejected (with the reason) or
late. That is how "22 rejected" becomes a bug the author can find. The
bot's own output is its own business: it runs in the author's terminal.

**Watch** a finished match:

```
$ pinch-points watch ~/.local/share/pinch-points/arena/2026-09-29-1402.replay
```

This opens the real game and plays the replay at full fidelity, with the
seat names and kinds on screen. The score says who won; watching is how
you find out why. Today the only way to see a replay is the
`PINCH_REPLAY` dev hook, which plays the last saved round, so `watch` is
new work, and it comes with the arena rather than after it.

## Cups

A **cup** is this design's word for a competition between bots. Not
"tournament": the game already uses that for a best-of-three or
best-of-five series (`app::tournament::Tournament`), and one word for two
things would confuse the code and the players both.

### Running one

The organiser starts a server. Registration opens:

```
$ pinch-points cup serve --listen 0.0.0.0:47710 --seeds 20 --seats 4 \
                         --add ai:hard
Cup t1: 20 beaches, 4 seats, deadline 33 ms, fair cursor off
Registration open (type `start` to begin). Entrants join with:

  pinch://192.168.1.20:47710/QW8R-3NDK
  + house (ai:hard)
  + bob      192.168.1.31   round trip 0.9 ms   4 games at once
  + alice    192.168.1.12   round trip 1.1 ms   1 game at once
  + carol    192.168.1.40   round trip 1.3 ms   2 games at once
> start
4 entrants, 1 table, 20 beaches, 4 rotations each: 80 games
[00:00:32] ======================== 80/80

  #  Bot     Points  Firsts  Avg score  Raided  Forfeits  Late  Reply p50/p99    RTT
  1  bob       2.05      38       38.2     1.2         0  0.1%   1.4 / 12 ms  0.9 ms
  2  house     1.64      24       31.0     1.9         0  0          -           -
  3  alice     1.38      15       24.7     2.8         0  2.4%   6.0 / 35 ms  1.1 ms
  4  carol     0.93       3       11.5     4.1         3  0.0%   2.2 / 9 ms   1.3 ms

Replays and logs: ./cup-t1/
```

`--seeds N` is how many beaches the cup is played on, each generated from
its own seed. `--add ai:<level>` enters the game's AI as a house bot, which
gives every field a fixed yardstick. `--start-when N` starts on its own
once N bots have registered, for a server nobody is watching.

A cup is played as a set of tables over a fixed list of seeds.

- **Rotation, not permutation.** At each table, on each beach, the seats
  rotate so every bot sits in every chair once: 4 games for a 4-seat
  table, not the 24 of every ordering. Seat bias is real (the balance
  harness has measured it), and a rotation is what cancels it; the order
  of the others around the table matters far less than whose chair is
  whose.
- **Tables are drawn, not enumerated.** With as many entrants as seats
  there is one table. With more, every possible table does not scale
  (10 entrants in 4 seats is 210 tables, 16,800 games over 20 beaches), so
  the server draws tables so that every pair of entrants shares a table as
  evenly as the game count allows, and every entrant plays the same number
  of games. `--tables N` sets how many tables are drawn per beach; the
  draw is seeded, and printed, so the schedule can be checked.
- **Points by finishing place.** In a four-player free-for-all there is no
  single "win", so each game pays by place: 3, 2, 1 and 0 for four seats
  (in general `seats - place`), a tie splitting the places it spans. The
  standings rank by average points per game, then by average score. A
  rating system (TrueSkill-style, which handles free-for-all) is the
  obvious next step for a long-running cup, and is left until a cup is
  long enough to need one.

The server runs as many games at once as the entrants' `parallel`
capacities allow. It takes the same `--deadline`, `--round`, `--map` and
`--fair-cursor` as the arena, and prints them in its header so the
standings say what they were played under.

**A disconnect costs games, never the cup.** A bot that drops idles
in the games it was in, and forfeits the ones that start before it
returns (see *Disconnects*). The forfeits are counted in the standings,
and the other games run as if nothing happened.

The output is the standings, per-bot statistics (banked, raided, rejected
placements, forfeits, late and missed decisions, round trip), every replay
and every log, in one directory. The final goes on the big screen with
`watch`. Because the sim is deterministic, anyone can re-run a disputed
game from its seed and the same bots, and the replay settles it either
way.

### Who may enter

The limits under *Hardening* apply to a cup server as to every listener.
What a cup adds is the question of who gets in, and how many times.

- **Invite keys.** A cup draws an invite key by default and puts
  it in the string it prints, so registration needs the string, not just
  the port. `--open-registration` leaves it out for a party where anyone
  on the LAN is welcome; on the internet the key is how the organiser
  decides who enters, instead of whoever finds the port.
- **Collusion is the threat a free-for-all invites.** A shared invite lets
  one author register five copies of a bot, and copies at one table can
  play as a team: feeding one castle, raiding the rest. Scoring by place
  rewards exactly that. So a cup knows every entrant's **owner**, and:
  - **One entrant per owner** by default (`--per-owner 1`). An organiser
    can allow more, for an author entering two different bots.
  - **The draw never seats two bots of one owner at the same table.** If
    the field is too small to keep them apart, the cup refuses to start
    and says why, rather than quietly scheduling a table that can collude.
- **Where the owner comes from.** `invite NAME` at the cup's console (or
  `--invite NAME` when it starts) prints a per-author string, bound to that
  owner, and a bot registering with it has the
  owner the organiser gave it, whatever it claims. With the shared invite
  or open registration the owner is the one the bot declares, which is
  trust among friends and is labelled as such in the standings. A cup
  that matters uses per-author invites.

## Phases

Each phase is useful on its own.

1. **Seat controllers.** Gather `Bots`, the cursors and the wire behind
   `SeatController`, with no behaviour change. The test that matters is
   the existing online AI-fill guard, which must still hold.
2. **Protocol and the arena.** The TCP listener side of the protocol
   (connection strings, join keys and tokens, registration, games, ticks,
   replies, `simulate`, disconnects, rejection reasons, the hardening every
   listener shares, `TCP_NODELAY` and reply-time measurement), the one-tick
   deadline with fast-forward and `input_delay` in `hello`, `--trace`, the
   `arena` subcommand with its flags (`--watch` and `--games` included),
   `watch` for a replay file with seat kinds in it, a Python example bot
   that beats Easy, and the written protocol spec for bot authors. This is
   the first thing a bot author can use, and every step of their loop has a
   command.
3. **Cups.** `cup serve`: the table draw, rotated seats, parallel games,
   forfeits, points by place, standings, replay fetching, invite keys,
   owners with `cup invite`, one entrant per owner, and a draw that keeps
   an owner's bots apart. This is the goal, and the first phase an
   organiser can use.
4. **The fair cursor**, with its `move` action and its reply rules, for
   bots and the game AI at once, with the balance budgets and the
   difficulty ladder re-measured in the same phase. It comes before any
   human shares a table with a bot.
5. **Join as bot.** Route 1: bot seats in couch play and "Join as bot" in
   the lobby, the seat kind on the wire, the robot icon and the owner's
   name everywhere a name is drawn, the game AI standing in for a dropped
   bot, "Bots welcome" in the beacon, kicking, and an "open replay"
   entry in the game's menu for players who never touch a terminal.
6. **Straight to the host.** Route 2: the host speaking for several seats
   on the wire, and the single-use connection string on the hosting card.
7. **Later, if wanted.** Watching a cup live: a game client joins
   the server as a spectator and follows any game in progress. The LAN
   spectator mode (`NetMsg::Watch`) is the model for it.

## Not in scope

- **Running anyone's bot.** Not now and not later: see *The one rule*.
- **A permanent online league** with accounts, seasons and a web
  leaderboard. A cup server on the internet, behind a proxy and an
  invite code, gets most of the way there; the rest is a web project, not
  a game one.
- **A bot holding several seats in one game.** One registration, one seat
  per game. A bot that wants to play itself registers twice.
- **Rollback.** Bots do not need it any more than humans do; see the
  backlog.

## Open questions

- **Where the first cup runs.** The design covers a LAN and the
  internet, and builds the LAN case first, since it is a subset of the
  internet one and matches how the game is played today. The answer moves
  the defaults: deadline, invite keys, forfeit timeout.
- **Cursor travel.** Does the virtual cursor walk one axis at a time
  (Manhattan) or diagonally, as a player holding two keys can? It should
  match what a human can actually do.
- **Visible countdowns.** The screen does not show `takeoff_in`, so the
  observation leaves it out. Any other state the screen keeps quiet about
  should be listed and held to the same rule, by a test if possible.
- **Bots in the chat.** A bot saying "gg" or "nice save" is fun and cheap:
  a `chat` reply, rate-limited, drawn with the robot icon. It is also a
  way to spam a lobby full of kids. If it is built, it sits behind a host
  switch of its own, separate from "Bots welcome".
- **How a cup is scored and drawn.** The design picks balanced drawn
  tables and points by place (3, 2, 1, 0) as the simplest thing that is
  fair. The alternatives are a Swiss draw, where later tables pair bots
  with similar records, and ranking by a rating from the first cup. Both
  are better for big fields and worse for being explained on a whiteboard
  at a party.
- **Protocol stability promise.** How long an old protocol version stays
  supported once version 2 exists.

## As built

What the build settled that the design left open, and where it parted
from it:

- **The fair cursor walks diagonally**, as a person holding two arrow
  keys does, and moves like one: the first tile at once, the second after
  the lift (8 ticks, the 0.28 s repeat delay), then one every 3 ticks. One
  statement in the sim (`fair_walk`) for the game's AI and a bot's virtual
  cursor alike. Every AI level now walks at that one pace (Hard used to be
  faster than any person); the ladder and the seat budgets were re-measured
  and did not move.
- **The game's AI walks under the fair cursor in every match the game
  sets up**, since every one has a person in it. In the arena and a cup
  with the rule off it has an instant hand, as the bots do.
- **A bot busy with a tick is not sent the next one.** The design sent
  every tick; a bot slower than its deadline then sank under boards it
  would never read. It still holds each tick its deadline, so it plays a
  tick behind, and the ticks it misses are skipped as if it had waited.
- **The flood allowance grows with every line sent to a bot**, so a fast
  bot answering thousands of fast-forward ticks a second is not a flood.
- **A bot's seat is called "Greedy (Ana)"**, not "Greedy (Ana's bot)": a
  name on the wire is twelve characters, and the robot beside it says the
  rest on every screen.
- **Seat controllers**: `Local` carries no input binding (the keyboard and
  pads still deal seats from P1 up), and `MatchConfig` keeps its seat and
  AI counts beside a controller per seat, for the same reason.
- **`cup invite NAME`** is `invite NAME` at the cup's console, or
  `--invite NAME` when it starts: an invite is drawn by the running server.
- **Kicking** is in the lobby, before a match: K, then the seat's number.
- **The "open replay" entry** is a replay file dropped on the window, on
  the menu or the replay shelf, which says so: the game has no file dialog.
- **The route-two string** shows under the host's Bots dial rather than
  on a card of its own.
