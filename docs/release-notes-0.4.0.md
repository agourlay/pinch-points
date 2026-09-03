# 0.4.0

A full read of the codebase, then the fixes it turned up. No new levels and
no new systems: 0.4.0 is the game playing the way it already claimed to,
online and off, with a round of chrome and performance work on top.

## What's new

- A Monopoly tide is scored as banking again. The event sweeps half the
  loose crabs into a castle, and the render layer read every one of them as
  a gull's work: a measured tick reported eighty-two crabs eaten, with
  eighty-two death sounds, the camera shake pinned at its cap, and no hop,
  no castle bounce and no floating score for the seat that had just taken
  half the beach. The sim writes down what it did, so the same tick now
  reads as eighty-seven banked and nothing eaten.
- The results card no longer bends the table. A player who arrived while the
  host sat on the scores was seated nameless, a watcher was dealt a chair, a
  peer that quit there became a ghost seat that froze the next round for
  five seconds, the call for the next round could put a joiner two frames
  ahead and desync the table on its first hash, and the beacon left the
  queue out of its count, so a full beach still advertised room.
- Four ways to crash the sim are refused with a reason instead: a gull
  placed on kelp, an arena smaller than 5x5, a signpost rule that must evict
  with nothing to evict, and a running lure, mania or tempo with zero ticks
  left. Level files, pasted round codes, a beach off the wire and the editor
  all ask the same rules now.
- A stage plays under what its file says. Castle raids had no line in the
  level format, so every editor-made puzzle was played, hinted and certified
  with raids on while every campaign puzzle plays without; and a stage
  asking for a ninety-second round was still judged on the campaign's
  sixty-second limit, so a Survive stage was won thirty seconds early.
- The last three list screens are drawn on the shared card. Settings, the
  trophies and the stage grid each built their own out of the same parts and
  each drifted: no drop shadow on any of them, three different paddings, and
  the trophies card centred in a frame two pixels off every other screen.
  The settings columns start on the same line, list rows are set at the size
  the type scale names, and the feed card keeps the fill the sidebar gave it
  instead of showing sand through it.
- The controls screen fits in every language. Seven labels were clipped in
  French, Spanish, German and Dutch, under a comment claiming the cells were
  sized for the longest. They are measured in pixels now, with a test
  holding them there.
- Less work per frame: the board is diffed only when something wrote to it,
  the hint ghost is redrawn when it moves rather than on every tick, the
  footfall sweep is a set lookup rather than a scan per crab, the stage
  list's progress lookup is built once per list instead of once per tile per
  frame, and the solver memoises its leaves.

## Heads up

- Online play needs 0.4 on both sides. The protocol byte moved to 10, so a
  0.3 build is turned away at the handshake rather than desyncing later.
- Kept rounds replay exactly as before. No tick outcome and no state hash
  moved in this release, so a 0.3 replay plays back bit for bit, and a build
  with the Monopoly fix and one without still agree on every frame of a
  round. They differ only in what they draw.
- Lifetime stats counted a Monopoly sweep under "crabs fed to gulls" and
  never under "banked". New rounds count it right; totals already on disk
  are not rewritten.
- A custom stage saved before 0.4 keeps its castle raids until you open it
  in the editor and save it again. The editor now turns raids off for a
  stage and writes `raids: off` into the file, and that line is what a
  reader goes by.
- An online round on a handmade beach with fewer castles than there are
  seats now falls back to a generated arena. It used to play, with the
  seats past the last castle unable to bank anything all round and nothing
  on any screen saying why.
- The level format gained one line, `raids: off`, written only when raids
  are off. Older files still read.
- A level or round code that would have crashed the sim is refused now, with
  the reason and, for a bad map row, the row number. Nothing the game itself
  has ever written falls foul of this.

## Also fixed

**Online**

- A peer can only speak under the name it greeted with. Chat is the one
  message that names its own sender, and the host was passing the claim on
  untouched, so a peer could wear a rival's name or the empty one, which is
  the room itself talking.
- A pause or resume frame off the wire is held to the same horizon input
  frames are. A Resume naming the last frame in the window made the next
  Escape propose a frame nobody would honour: a panic in debug, and in
  release a round stalled for good.
- A beacon too short to carry a kind byte read byte 7 of whatever packet
  came before it, so an older host's beach could be listed as a round
  already under way: greyed out, with nobody able to join.
- A joiner told of its own abandonment logged that it had given up on itself
  and sat on a frozen beach for twenty seconds. It reads its own seat in the
  notice now and goes back to the menu saying what happened, in all eight
  languages.
- The Start decoder refuses a datagram that ends before the beach length,
  and the LZW decoder refuses a code past the entry being built, as its own
  documentation always promised.

**In a round**

- Escape in a running puzzle was read twice, by the phase and by the pause
  card. The first froze the round, the card kept that frozen state as the
  one to hand back, and Continue left the beach running with no card up.
- A gull and a crab passing head-on through the seam of a wrapping arena
  walked through each other, and a lured crab never took the short way
  round: both measured raw coordinates where the sand had them a tile apart.
- The pad cursor never sped up when held. Every step waited the full first
  delay.
- Sand puffed under every gull a level was written with, on load and on
  every retry.
- A pasted round further along than the last board watched opened with a
  burst of phantom events, every remembered crab departing into whatever the
  new beach had at its old tile.
- A resumed round seated two keyboard cursors whatever its table said, so a
  keypress on the AI's keys steered its crabs over the bot's head. The daily
  challenge wrote its own table into the player's match config, which came
  back on the setup screen as their choice.
- The camera fitted the postcard screens to whatever board the sim last
  held, so after a round the beach behind the settings card stopped short of
  the window.
- Leaving a puzzle mid-run left its phase where it was, so the next puzzle
  ticked the stale board once before the load swapped it.
- A pasted round is filed on the shelf under whoever took it, like every
  other round there, rather than under the beach's name.
- P in match setup wrapped onto the end of the list without asking the
  ladder, which N two lines up already asked. It asks now, and flashes the
  same refusal.

**Sound**

- A castle taking a whole stream is one sound, not one per crab. Twelve
  six-seat bot rounds put eighty-seven bank cues on a single tick: at the
  spatial gain that is one blare and eighty-seven sinks mixing it. Deduped
  per castle per frame, so six castles filling at once are still six things
  in six places.
- Signposts are heard by a watcher and in a replay. The seat rule read
  "nobody" for a machine that answers for no seat, so every post fell silent
  for an online spectator and in every replay while the banks, the gulls and
  the horn played on.
- The theme no longer speeds up across the final scramble. The ramp held
  its top speed through the whole results card and then snapped back to
  normal the frame the screen changed, which read as the music slowing
  down at the end of the round. The scramble still announces itself with
  the doubled gulls, the reddening water, the red clock and the surge
  stinger.

**Screens**

- A bank is worth one floating number, summed over the frame. Eight banked
  crabs put eight identical "+1"s pixel on pixel, which read as a single
  slightly dark "+1", and the floating number is the only place a bank's
  worth is ever shown. It says "+8" now.
- The cursor's gold wash has one name and one definition, where there were
  three spellings of the same three numbers.
- The reserved strip at the top of every list screen is derived from the
  header bar that fills it, rather than being a second number in another
  file that nothing tied to the first.
- Two editor lines read wrong at one arrow or one hole in seven languages;
  they count noun then number, the way the rest of the game counts.
- Floating text honours the alpha it asked for, and the sidebar clock spawns
  in the colour it will be drawn in.

**Housekeeping**

- A release address handed to a browser, or on Windows to `cmd`, may no
  longer carry a percent: `cmd` expands `%NAME%`, and a release page of ours
  has never needed an escape.
- The release workflow checks that the tag names the version in Cargo.toml
  before it builds. The start-up update check compares the two, so a tag
  ahead of the manifest makes every build of that release nag itself
  forever.
- The README names the eight `PINCH_*` development hooks it was missing and
  the editor's F6, `levels/06` is called Undertow on disk as well as in its
  header, and a handful of module docs point at files that have existed
  since the test targets were merged.
- The editor certifies the level text it is about to save rather than the
  board on screen. The two differ once a gull has been placed and erased,
  since the text carries only the seed, so "solvable" is now a claim about
  the beach that ships.
