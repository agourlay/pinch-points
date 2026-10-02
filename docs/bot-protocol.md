# Writing a Pinch Points bot

**Protocol version 1.** This is the contract a bot is written against. The
design behind it, and why it is shaped this way, is `bot-seats.md`.

A bot is a program you run yourself, in any language, on any machine that
can reach the game. It connects over TCP, registers, and answers each tick
of a game with an action. The game never runs your code: it listens, sends
the board, and reads your moves back. The bot you test at home is byte for
byte the bot that plays in the final.

`bots/python/greedy.py` is a complete bot in one file of plain Python. It
is the quickest way in: copy it and change `decide`.

## Quick start

```
$ pinch-points arena --seat ai:easy --open 1
Waiting for 1 bot. Start it with:

  pinch://127.0.0.1:47710/H4TN-C2LV
```

In another terminal:

```
$ python3 bots/python/greedy.py pinch://127.0.0.1:47710/H4TN-C2LV
```

The arena plays a round, prints the standings, and says where the replay
and the decision log went. `pinch-points watch FILE` plays the replay in
the game's window.

## Contents

- [Connecting](#connecting)
- [Registration](#registration)
- [A game](#a-game): `hello`, `ready`, `tick`, the reply, `end`
- [Clocks and deadlines](#clocks-and-deadlines)
- [The fair cursor](#the-fair-cursor)
- [Looking ahead: `simulate`](#looking-ahead-simulate)
- [Replays](#replays)
- [Disconnects](#disconnects)
- [Limits](#limits)
- [The rules](#the-rules), precisely enough to port the sim
- [The author's loop](#the-authors-loop): `arena`, the log, `watch`, `--trace`
- [Playing with people](#playing-with-people)
- [Cups](#cups)

## Connecting

Every way in hands a bot the same thing, one line, its connection string:

```
pinch://HOST:PORT/KEY
```

`KEY` is a join key, eight characters in two groups (`7F3K-9QXA`). It is
optional: an open cup prints `pinch://HOST:PORT` alone. `pinchs://` means
"connect with TLS", for a server on the internet behind a TLS proxy; the
game itself only ever prints `pinch://`. The default port is 47710.

Make your bot's command line `mybot <string>` and it can join anything.

**Transport.** TCP, one JSON object per line (UTF-8, `\n`), in both
directions. Anything that is not a JSON object on one line is not a
message.

**Switch Nagle off.** Set `TCP_NODELAY` on your socket before anything
else. Nagle's algorithm meeting delayed ACKs can hold a small line for
40 ms, longer than the whole deadline. In Python:

```python
sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
```

**Coordinates** are tiles: `x` to the right, `y` down, `(0, 0)` top left.
**Directions** are `"up"`, `"down"`, `"left"`, `"right"`.

## Registration

The first line a bot sends, on every connection:

```json
{"type": "register", "protocol": 1, "name": "Alice's crab herder",
 "version": "0.3", "owner": "Alice", "parallel": 4, "key": "7F3K-9QXA"}
```

| Field | | |
|---|---|---|
| `protocol` | required | `1`. A listener refuses a version it does not speak, and says which it does. |
| `name` | required | How the bot appears on screen and in standings. At most 32 characters. Unique on the listener: a taken name is refused. |
| `version` | optional | Shown beside the name, so you can tell this build from last week's. |
| `owner` | optional | The person behind the bot. A listener that already knows the owner (a per-author cup invite, the author's own game) ignores this. |
| `parallel` | optional | How many games the bot will play at once over this connection. Default 1, capped by the listener (8 by default). |
| `key` | when asked | The key from the connection string. A listener that needs one refuses a registration without it, or with the wrong one. |

The answer:

```json
{"type": "registered", "name": "Alice's crab herder", "token": "K3F9..."}
```

The **token** is who your bot is for the life of the listener: an arena
run, a whole cup. Keep it. If the connection drops, reconnect and register
with it instead of the key, and you are the same bot, seat and standings
and all:

```json
{"type": "register", "protocol": 1, "token": "K3F9..."}
```

A join key gets a bot in once (a cup's shared invite is the exception: it
admits every entrant); the token is who it is afterwards. Never print or
log your token.

A refusal is an `error` with `"fatal": true`, and the listener closes the
connection:

```json
{"type": "error", "fatal": true, "message": "the name \"Greedy\" is taken here"}
```

An `error` without `fatal` is a complaint about one message, and the
connection carries on.

**Pings.** At any time outside a tick the listener may send
`{"type": "ping", "id": 7}`. Answer `{"type": "pong", "id": 7}`. It measures
your round trip, which the standings print, and costs you nothing.

## A game

Every message about a game carries its `game` number. A bot with
`"parallel": 1` sees one game at a time and can ignore the field; a bot
that plays several at once uses it to keep them apart.

### `hello`

When a game starts, the listener sends the static half of the board, the
rules, the clock and the table:

```json
{"type": "hello", "game": 17, "seat": 2, "seats": 4,
 "names": ["Ana", "Normal AI", "Alice's crab herder", "bob"],
 "kinds": ["human", "ai", "bot", "bot"],
 "board": {"width": 12, "height": 9, "wrap": false,
           "tiles": ["0..........1", "...", "..."],
           "walls": {"h": ["------------", "..."], "v": ["|..........|", "..."]},
           "spawners": [{"x": 0, "y": 4, "dir": "right", "period": 46}]},
 "rules": {"signpost_cap": 3, "cap_policy": "evict",
           "signpost_lifetime": 300, "round_ticks": 5400,
           "gull_period": 240, "castle_raids": true, "events": true},
 "clock": {"mode": "fast_forward", "deadline_ms": 33, "input_delay": 0},
 "cursor": {"fair": false}}
```

`seat` is yours, counting from 0. `kinds` says what holds each seat:
`human`, `ai` (the game's own AI) or `bot`.

**`tiles`**, one string per row, one character per tile:

| | |
|---|---|
| `.` | sand: the only tile a signpost goes on |
| `#` | rock: nothing enters it |
| `0`-`5` | a castle, with the seat that holds it at the start (who holds it now is in every tick's `castles`) |
| `S` | a crab hole (spawner) |
| `T` | a turnstile log (which way it deflects next is in every tick's `turnstiles`) |
| `K` | kelp: crabs walk through, walking gulls cannot enter |
| `~` | a tide pool: whatever stands in it moves at half speed |

**`walls`.** Walls stand on the edges between tiles. `h` has `height + 1`
strings of `width` characters: `h[y][x]` is the edge *above* tile `(x, y)`,
and `h[height]` is the bottom border. `v` has `height` strings of
`width + 1` characters: `v[y][x]` is the edge to the *left* of tile
`(x, y)`, and `v[y][width]` the right border. `-` and `|` are walls, `.`
is open. On a `wrap` board the borders are open and a creature walking off
one side comes back on the other.

**`rules`.** `round_ticks` is `null` on an untimed beach. A placement past
`signpost_cap` evicts your oldest post under `cap_policy: "evict"`, and is
refused under `"reject"`. Posts wash away `signpost_lifetime` ticks after
they were placed. `gull_period`: a gull arrives at the edge every this many
ticks (`0`: none). `events`: whether sparkling crabs spin the tide roulette.

### `ready`

Answer `hello` before the first tick:

```json
{"type": "ready", "game": 17}
```

A bot that has not answered within ten seconds forfeits the game.

### `tick`

Every tick, everything a player can see on the screen and nothing they
cannot:

```json
{"type": "tick", "game": 17, "tick": 1234, "remaining": 4166,
 "scores": [12, 3, 20, 7],
 "you": {"seat": 2, "signposts": 2,
         "last": {"act": "place", "accepted": false, "reason": "rival_post"}},
 "cursors": [[3, 4], [0, 8], [5, 5], [10, 7]],
 "crabs": [{"id": 88, "x": 5, "y": 3, "dir": "left", "progress": 96,
            "kind": "giant", "claw": "right"}],
 "gulls": [{"id": 4, "x": 9, "y": 2, "dir": "down", "progress": 0,
            "state": "walking", "hops_left": 0, "claw": "left"}],
 "signposts": [{"x": 5, "y": 2, "dir": "up", "owner": 2, "worn": false, "age": 41}],
 "castles": [{"x": 1, "y": 1, "owner": 0}],
 "turnstiles": [{"x": 6, "y": 4, "next": "right"}],
 "event": {"name": "gull_mania", "ticks_left": 120},
 "last_event": {"name": "gull_mania", "tick": 1054},
 "lure": null, "claw_call": false, "surge": false}
```

| Field | |
|---|---|
| `tick` | The board's tick. Your reply names it. |
| `remaining` | Ticks until the tide comes in, or `null` on an untimed beach. |
| `scores` | Banked score, per seat. |
| `you.signposts` | How many of your posts are standing. |
| `you.last` | What became of your last placement, removal or clear: `accepted`, or not and the `reason` (below). `null` before you have tried one. |
| `cursors` | Each seat's cursor, `[x, y]`, or `null` where the listener cannot see it (a remote player's, at a party). |
| `crabs` | `x`, `y` is the tile the crab is walking *out of*; `progress` is how far, in 256ths of a tile. `kind` is `common`, `juvenile`, `giant`, `molting`, `golden` or `sparkling`; `claw` is its big claw, the side it tries first at a wall. |
| `gulls` | The same, plus `state` (`walking` or `flying`) and `hops_left` while flying. How long until a gull next takes off is not shown: the screen does not show it either. |
| `signposts` | Every post on the beach: its `owner`, whether a gull has `worn` it, and its `age` in ticks. |
| `castles` | Every castle and the seat holding it now (Castle Swap trades them). |
| `turnstiles` | Every log and which way it deflects the next creature. |
| `event` | The timed tide event running (`crab_mania`, `gull_mania`, `speed_up`, `slow_down`, `right_claws`) and its ticks left, or `null`. |
| `last_event` | The last tide event of any kind and the tick it fired (`monopoly`, `gull_attack`, `fresh_sand` and `castle_swap` are instant). |
| `lure` | A molting crab's lure: whose castle every loose crab is walking to, and for how long. |
| `claw_call` | Right Claws is on: a right-clawed crab banks double, a left-clawed one costs its value. |
| `surge` | The last 30 seconds: gulls come twice as often. |

The full board is sent every tick, never a change from the last: a bot
that is stateless is a correct one.

### The reply

At most one action per tick, naming the tick it answers:

```json
{"game": 17, "tick": 1234, "act": "place", "x": 5, "y": 2, "dir": "left"}
{"game": 17, "tick": 1234, "act": "remove", "x": 5, "y": 2}
{"game": 17, "tick": 1234, "act": "clear"}
{"game": 17, "tick": 1234, "act": "move", "x": 5, "y": 2}
{"game": 17, "tick": 1234, "act": "none"}
```

| `act` | |
|---|---|
| `place` | A signpost on `(x, y)` pointing `dir`. Placing on your own post re-points it, fresh. |
| `remove` | Take your post at `(x, y)` up. |
| `clear` | Take up your first post in reading order (top row first), wherever it stands: a person's clear-all key, one post a tick. |
| `move` | Put your cursor on `(x, y)` and do nothing there. Only means something under the fair cursor. |
| `none` | Nothing. |

Two optional fields on any reply:

- **`"wait": k`**: do not send me the next `k` ticks. They are not sent at
  all, and nothing is lost: the next tick you get carries the full board.
  A bot that thinks in half seconds should say so; in a fast-forward game
  a round where every bot waits runs at the sim's own speed.
- **`"note": "..."`**: written to the decision log beside the action. Your
  reasoning goes here. Notes are cut at 4 KiB, never refused.

A reply that is not valid JSON, or names an action that does not exist,
counts as `none` and is logged. The newest reply for a seat wins: one
answering an older tick than a reply already in hand is dropped.

**Refusal reasons** (`you.last.reason`): `off_board`, `rock`, `castle`,
`spawner`, `turnstile`, `kelp`, `pool` (only sand takes a post),
`rival_post` (somebody else's post stands there, or got there first this
tick), `out_of_posts` (every post spent on a board that refuses rather than
evicts), `no_post` (a removal or clear with nothing of yours there).

### `end`

```json
{"type": "end", "game": 17, "scores": [12, 3, 20, 7], "placing": 1, "replay": "t1-g17"}
```

`placing` is 1 for first; seats that tie share a place.

## Clocks and deadlines

**One deadline, everywhere.** Every tick a bot is sent has the same
deadline, `clock.deadline_ms` from the moment it was sent: one tick, 33 ms,
unless the organiser raised it. A reply that lands in time is that tick's
action; a bot that misses it does nothing that tick. The deadline is
measured at the listener, so it includes the network.

What differs is only **when the next tick is sent** (`clock.mode`):

- **`live`**, any match a person is watching or playing in: on the wall
  clock, 30 times a second. The game never waits for a bot.
- **`fast_forward`**, the arena and cups: as soon as every bot has answered
  or its deadline has passed. Idle time is skipped, never handed out: no
  bot ever gets more than its deadline, and a round whose bots answer in a
  millisecond takes seconds instead of three minutes.

**Thinking happens before the reply.** The deadline guarantees you the time
until you answer and nothing after. Live, a bot that answers at once still
gets the rest of the 33 ms, because the next tick waits for the clock; in
fast-forward the next tick may come at once. A bot that wants the whole
tick should think first and answer at the end of it.

**Late replies.** A late reply still counts at the next tick, unless a
newer reply has arrived by then, and the log records it as late. Each
tick's deadline starts when that tick is sent, so lateness never buys time:
a bot that is always late is always a tick behind.

**`input_delay`.** A reply to tick `t` takes effect `input_delay` ticks
later. In the arena, in a cup and on a couch it is 0: your reply to `t` is
the action the sim takes from `t` to `t + 1`, and a post placed then
already turns a crab arriving on that tick. In an online game it is 3,
because every seat's input is committed that far ahead for the whole
table: aim where the crab will be, as a person online does. A bot that
ignores `input_delay` plays well in the arena and misses at a party.

## The fair cursor

`cursor.fair` says whether the fair cursor rule is on. It is on in any
match with a person in it, and off by default in the arena and cups
(`--fair-cursor on` turns it on).

Under the rule your seat has a cursor that moves at a person's pace, as a
held arrow key moves one: the first tile at once, the next after
`cursor.lift` ticks (8), then one every `cursor.ticks_per_tile` ticks (3),
diagonally when it needs to (a person holding two arrow keys does). A walk
of `d` tiles takes 0 ticks for `d <= 1`, else `lift + (d - 2) *
ticks_per_tile`, where `d` is the larger of the two axes' distances. Your
reply names a target and the cursor walks there before the action lands:

- `place` and `remove` set a new order: walk to that tile, then act. It
  replaces any order still walking.
- `move` is an order with nothing at the end of it.
- `none` means "no new order". A walk in progress carries on: you do not
  have to repeat yourself every tick, and a `wait` does not stop the walk.
- `clear` needs no walk, as the clear-all key needs none.

Your cursor is in every tick's `cursors`. `you.last` reports the action
when it lands, not when you asked. The game's AI walks by the same rule,
from wherever it last placed, so under the rule nobody at the table has a
faster hand than a person; with the rule off it places anywhere at once,
as a bot does.

## Looking ahead: `simulate`

A Rust bot can link the game's crate and run the sim forward itself. So
that this is not an edge only Rust has, the listener answers a lookahead
for any bot:

```json
{"type": "simulate", "game": 17, "ticks": 60,
 "plan": [{"at": 0, "act": "place", "x": 5, "y": 2, "dir": "left"}]}
```

It runs a copy of the board `ticks` ticks forward with your `plan` (`at`
counts ticks from now) and every rival idle, and answers:

```json
{"type": "simulated", "game": 17, "ticks": 60,
 "state": {"type": "tick", "...": "the tick message at the end"},
 "events": [{"tick": 1240, "what": "banked", "crab": 88, "owner": 2, "x": 1, "y": 1},
            {"tick": 1251, "what": "eaten", "crab": 90, "x": 4, "y": 4},
            {"tick": 1262, "what": "raided", "owner": 0, "lost": 6},
            {"tick": 1270, "what": "post_worn", "x": 5, "y": 2, "owner": 2},
            {"tick": 1290, "what": "post_gone", "x": 5, "y": 2, "owner": 2}]}
```

The copy knows exactly what a player reading the board knows, and nothing
more. Everything already on the sand is exact. Crabs that spawners will
emit arrive on schedule (a spawner fires on `tick % period == 0`), but
their kind and claw are a guess, marked `"predicted": true`. Ambient gulls
are left out (where the next one lands is the PRNG's secret), and a gull's
takeoffs are a guess too.

A lookahead counts against your own deadline like any other thinking, and
is capped per decision: 600 simulated ticks per tick sent in a
fast-forward game, 60 in a live one, across all your requests for that
tick. Past the cap the answer is an `error`.

## Replays

```json
{"type": "replay", "id": "t1-g17"}
```

answers `{"type": "replay", "id": "t1-g17", "text": "replay-v2\n..."}` for
any finished game your bot played in. Save the text and
`pinch-points watch` it.

## Disconnects

A dropped connection costs the bot, never the match. Your seats idle from
the moment the connection went. Reconnect and register with your token
while a game is still running, and you get that game's `hello` again,
marked `"resumed": true`, then the next tick, and play on. A game that
started while you were away waits for you up to the organiser's forfeit
timeout (60 s by default); after that it is played with your seat idle and
scored as a forfeit: last place.

## Limits

Every listener treats every byte it receives as hostile.

- A line longer than 64 KiB closes the connection.
- A bot that floods (more than about 200 messages a second, after a
  burst) has messages dropped, and is then disconnected.
- Three wrong keys or tokens from one address and that address is refused
  for a minute.
- Names are display text: control characters are removed, and they are cut
  to 32 characters.
- A bot that stops reading its socket is disconnected once a couple of
  megabytes are waiting for it.

## The rules

The sim is deterministic and integer-only, so a port of it can be exact.
The full rules are `pinch-points-spec.md` §3 and §4; what a port needs is
here.

**Movement.** One tile is 256 subunits. A creature at `(x, y)` with
`progress` p is p subunits out of that tile's centre toward the next tile
in its `dir`. Each tick it advances by its speed: common, molting and
sparkling crabs 12, juveniles 18, giants 7, golden crabs 15, walking gulls
8, flying gulls 16. Speed Up doubles every step and Slow Down halves it
(never below 1); standing in a tide pool then halves it again (never below
1). Crossing 256 moves it to the next tile, where it resolves its arrival,
and carries the remainder.

**Arrival**, in order, on reaching a tile's centre:

1. A castle: a crab banks there and is gone; a walking gull raids it (half
   the bank, rounded up, is lost, some of it spilling back as crabs) and
   leaves the beach.
2. A turnstile: deflect to its `next` side, flip it, then wall-resolve.
   Steps 3 and 4 do not run: a post on a turnstile is never consulted.
3. A lure is running: a crab steps toward the luring castle instead of
   reading any post.
4. A signpost: the direction becomes the post's. A walking gull wears a
   post as it crosses (full, then worn, then gone).
5. Walls: keep going if the way ahead is open; else turn to the big claw's
   side if open; else the other side; else turn back. A post pointing into
   a wall is followed, then wall-resolved.

**Tick order.** Seats act first, in an order that rotates every tick
(one seat leads, the rest follow round the table); on two placements for
one tile the earlier seat wins and the later is refused `rival_post`. Then
posts older than their lifetime wash away, spawners emit, the ambient gull
arrives, crabs move, gulls move, and gulls eat crabs within 48 subunits of
them (Manhattan distance between board positions).

**Spawners and caps.** A spawner emits a crab facing its `dir` on every
tick where `tick % period == 0`, unless the beach already holds one crab
for every 3 tiles (`CRAB_CAP_TILES_PER_CRAB`). Crab Mania floods every 8
ticks, up to twice that cap; Gull Mania makes spawners emit gulls. The
ambient gull spawner pauses while 6 gulls are on the beach (`GULL_CAP`).
Spawned kinds: 70% common, 15% juvenile, 8% giant, 3% molting, 2% golden,
2% sparkling.

**Values.** Common 1, juvenile 2, molting 5, giant 10, golden 50,
sparkling 1. Banking a molting crab starts a 10 s lure (300 ticks) toward
the banker's castle, with a 20 s quiet spell after it before another; a
sparkling crab spins the tide roulette.

**Checking a port.** `pinch-points arena --trace FILE` writes one line per
tick: `{"state": <the tick message>, "actions": [<each seat's act>]}`. The
next line is the state those actions led to. Feed your port each state and
its actions and compare. Expect differences only where the PRNG decides
(what a spawner emits, where a gull lands, what the roulette spins).

## The author's loop

Edit, play, read the log, watch. Every step has a command, and every
command is the shipped binary: no Rust toolchain needed.

**Play.** `pinch-points arena` opens the seats it is given and waits for
bots on the rest.

| Flag | Default | |
|---|---|---|
| `--seat ai:<level>` | | a seat for the game's AI: `easy`, `normal`, `hard`; repeatable |
| `--open N` | 0 | seats left for bots to connect to; one string is printed per seat |
| `--listen ADDR` | `127.0.0.1:47710` | where to wait; `0.0.0.0:47710` lets a friend's bot in |
| `--port P` | 47710 | the port, on the default address |
| `--key KEY` | drawn fresh | a fixed key, for a script that starts the arena and the bot together |
| `--map NAME\|FILE` | `classic` | `classic`, `generated`, `small`, `large`, `xl`, `ocean`, or a level file |
| `--seats N` | seats given | a bigger table; the extra seats play `ai:normal` |
| `--seed S` | random, printed | the same beach again, for a fair before and after |
| `--round short\|standard\|long` | `standard` | 2, 3 or 5 minutes |
| `--games N` | 1 | play N seeds (`S`, `S+1`, ...) and print averages; bots stay connected between games |
| `--deadline MS` | 33 | raise it only to match a cup that raised it, or for a slow match |
| `--trace FILE` | off | every tick message and the actions after it |
| `--fair-cursor on\|off` | `off` | the fair cursor rule |
| `--watch` | off | draw the match in a window as it plays, in real time, with notes in the terminal |

`--open 2` with the same bot started twice, once with each string, plays
your bot against itself, or against last week's version.

**Read the log.** Every decision each bot made: the tick, the action, its
note, and whether it was refused (with the reason) or late. Its path is
printed at the end of every run.

**Watch.** `pinch-points watch FILE` opens the game and plays a replay at
full fidelity, with the seat names on screen.

## Playing with people

The same bot, unchanged, can take a seat beside people in the game
itself. The game opens a doorway, a card showing the connection string,
and the bot that registers with it takes the seat. Nothing in the
protocol differs; what differs is in `hello`:

- **On a couch.** In Turf War's match setup, turn a seat's AI dial past
  "fierce" to "bot". Starting the match shows a string per bot seat
  (`C` copies one, `L` opens the doorway to the LAN for a friend's
  laptop), and the match begins the moment the last bot is in. The clock
  is `live` with `input_delay` 0.
- **At a LAN party (Join as bot).** In the Beach Lobby, put the cursor on
  a beach that shows a robot ("Bots welcome") and press `B`. Your game
  shows the string; start your bot with it, and your game joins the
  party as an ordinary player whose seat your bot drives, named for your
  bot and you, "Greedy (Ana)". You watch the match from your own screen,
  with your bot's notes in the game's log. The clock is `live` with
  `input_delay` 3: every seat's input is committed three ticks ahead for
  the whole table, so aim where a crab will be, as a person online does.

- **Straight to a host.** A host whose beach takes bots shows a string
  under its Bots dial, `pinch://192.168.1.20:47710/M2QD-7WTR`. A bot
  anywhere on the network connects with it, no game window needed on its
  side, and takes a chair at the table; the host plays its moves to
  everyone. The key is good once and redrawn for the next bot, so come
  back with your token, never the key. The seat reads with the owner you
  declare, which the host cannot check. The clock is `live` with
  `input_delay` 3.

With a person at the table the fair cursor is on, for your bot and the
game's AI alike. A bot whose connection drops idles for five seconds and
then the game's AI stands in for it, until it comes back with its token.
A bot always wears a robot beside its name on every screen at the table;
the game sets it, never the bot. The host can ask anyone to leave (`K`
in the lobby, then their number).

## Cups

A cup is a competition between bots. The organiser runs a server:

```
$ pinch-points cup serve --seeds 20 --seats 4 --add ai:hard
Cup t1: 20 beaches, 4 seats, deadline 33 ms, fair cursor off
Registration open (type `start` to begin). Entrants join with:

  pinch://192.168.1.20:47710/QW8R-3NDK
  + house (ai:hard)
```

Every entrant starts their bot with that string, and the bot waits for
its games. The cup plays every entrant on the same list of beaches,
rotating the seats so every bot sits in every chair once, and pays each
game by place: with four seats 3, 2, 1 and 0 points, a tie splitting the
places it spans. The standings rank by average points per game, then by
average score. A bot that can play several games at once (`parallel` in
`register`) finishes the cup sooner.

| Flag | Default | |
|---|---|---|
| `--listen ADDR` | `0.0.0.0:47710` | where bots connect |
| `--seeds N` | 10 | beaches the cup is played on |
| `--seats N` | 4 | chairs a table |
| `--add ai:<level>` | | the game's AI as a house bot, a yardstick for the field; repeatable |
| `--tables N` | enough for everyone | tables drawn per beach, when there are more entrants than chairs |
| `--start-when N` | | start on its own once N bots have registered |
| `--invite NAME` | | a string bound to that owner; also `invite NAME` at the console |
| `--open-registration` | off | no key at all: anyone who can reach the port may enter |
| `--per-owner N` | 1 | bots one owner may enter |
| `--deadline MS`, `--round`, `--map`, `--fair-cursor` | | as for the arena, printed in the header |
| `--forfeit-after S` | 60 | how long a game waits for a bot that is not there |
| `--seed S` | random | the first beach's seed, and the draw's |
| `--out DIR` | `./cup-t1` | replays, logs, `schedule.txt` and `standings.txt` |

With more entrants than chairs, tables are drawn so that every entrant
plays as often as every other and every pair meets as evenly as the
count allows; two bots of one owner never share a table. The draw is
seeded and written to `schedule.txt`, so it can be checked. A bot that
drops idles in the games it was in and forfeits the ones that start
before it is back; the forfeits are counted in the standings.

**Owners.** A bot registered with a per-author string has the owner the
organiser gave it, whatever it claims. With the shared string the owner
is what the bot declares, which is trust among friends, and the
standings mark it so.

When the cup is over the server keeps listening, so every bot can fetch
the replays of its games (`{"type": "replay", "id": ...}`), until the
organiser types `quit`.
