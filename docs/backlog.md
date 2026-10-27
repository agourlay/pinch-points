# Backlog

Remaining ideas and known follow-ups. Done work lives in git history, not
here, including the measurements behind it, which belong next to the code
they explain.

Audited against the tree on 2026-09-17. Entries are grouped by what they
are waiting for, which for most of them is a machine, or a reason, or
nothing.

Two habits keep this file honest. Delete an entry in the commit that
implements it: the ghost-post entry outlived its own implementation by
three weeks. And date a figure whenever one is written down, because a bare
number reads as current for ever: a level count, a tag and a crate version
had all aged out by the time anyone looked again.

## Ready when someone is

Nothing blocks these. They want the work and no more.

- **Balance harness in CI.** Run `examples/balance.rs` and
  `examples/ladder.rs` nightly and track seat drift and the difficulty
  ladder. Both are cheap, a minute for the pair, and both have caught real
  regressions: an inverted ladder once, and a seat handicap in the bot's
  blunder draw that had been in every round ever played. `ci.yml` has a
  single `build` job on push and pull request, and no schedule.

  That second regression is the argument for the nightly: the figures here
  were taken 2026-08-11 and not read again until 2026-08-22, by which time
  every one of them had moved, one from 3.2 sigma to 5.9. This entry then
  quoted the 2026-08-22 figures as current through a 2026-09-17 run that
  had already moved them, which is the same lapse one paragraph smaller.
  The numbers live with the budgets they are judged against now, in
  `examples/balance.rs`, and this entry does not repeat them. Standing at
  2026-09-18 (`BALANCE_FULL=1`), all four gated sweeps are inside budget.
  The 16x11 run is 200 games against the others' 3000, so its figure is
  the noisy one; a nightly job should even the sample sizes first.

  Read the generated sweeps and not the `classic` ones, which play a single
  handmade board a hundred times with only the warm-up offset varying: a
  small, heavily correlated sample whose sigmas swing several points between
  runs that change nothing they measure.

- **Bot seats**, so people can program bots and hold competitions between
  them, on the same beach as humans and the game's AI. A project with its
  own design, in `bot-seats.md` (proposal, 2026-09-29).

- **An itch.io page**, with the shots in `docs/screenshots`. The binaries
  exist to put on it: `v0.6.0` is tagged (2026-09-28) and `release.yml` builds six
  targets across Linux, Windows and macOS. A Steam build is a longer road
  and has a section of its own below; the update check is the one place the
  two disagree, wanting to be on for a downloaded build and off for a store
  one.

## Waiting on a machine

A one-line change whose whole difficulty is judging it, and it cannot be
judged on the machine it was written on.

- **Try a shorter input delay before buying rollback.** `DEFAULT_DELAY` is
  3 (`sim/net/mod.rs`), the floor of the delay a host picks per round from
  its players' round trips (`delay_for_round_trip`). On a wired
  LAN the round trip is about a millisecond, so most of those three frames
  are jitter margin, and delay 1 still leaves a 33 ms budget. Turning it
  down recovers two thirds of what rollback offers for the price of a
  constant; the session waits rather than desyncs, so too low trades
  latency for hitches, which wifi shows before ethernet does. Wants two
  machines and a real LAN to judge.

## Waiting on a reason

Buildable, understood, and not currently worth having. Each says what
would change that.

- **Rollback, on `ggrs` directly.** Nothing blocks it: `ggrs` 0.13 depends
  on no engine (versions re-checked 2026-09-17). What is missing is the
  need. Rollback's whole product is giving back the input delay, and that
  delay is 100 ms (three frames at 30 Hz), online only, since couch play
  ticks the sim directly, against a common crab that takes 710 ms to cross
  a tile. A seventh of a tile, on the mode fewer people play, over a link
  whose round trip is a millisecond. At 40-80 ms of real internet the delay
  has to grow to four or six frames and the waiting stops being rare; that
  is when prediction earns its keep. Try the shorter delay first.

  Skip `bevy_ggrs` whenever it does happen. It exists to roll back ECS
  component state, and the sim is deliberately outside the ECS (spec §7.2),
  so it is an integration layer for a problem this game does not have. Raw
  `ggrs` fits what is here already: `Config::State` is a `Board` (`Clone`,
  `state_hash`, `to_snapshot`), `Input` is the 3-byte packed action
  (`encode_action`; the 8 of `INPUT_BYTES` is the whole wire message),
  `Address` is a `SocketAddr`, and `transport.rs` is the socket. And
  `sim_events.rs` diffs the board once a frame rather than hooking the
  moments things happen, so a re-simulated frame emits the net change
  instead of firing every bank sound twice, which is the part that usually
  makes a rollback retrofit miserable.

  Price: `ggrs` wants serde on the input type, and brings serde, bincode,
  parking_lot and rand with it. All of that lands in the shell beside the
  transport, so `crate::sim` keeps its no-dependency rule.

- **Online lobby beyond LAN** (matchbox-style signalling), if the game ever
  leaves the living room, which cuts against what it is for. `transport.rs`
  speaks UDP to a `SocketAddr` given by hand, which is a LAN and nothing
  else. Reach past `bevy_matchbox` to `matchbox_socket`, which is
  engine-free and has an optional `ggrs` feature: the two pair without a
  Bevy wrapper in the middle.

  Worth keeping straight before reading either netcode entry: `ggrs` and
  `matchbox_socket` carry no Bevy dependency at all, while `bevy_ggrs` and
  `bevy_matchbox` are the wrappers that marry them to the ECS. It is the
  wrappers that lag Bevy releases, and the wrappers this game has least use
  for. As of 2026-09-17 and unchanged since 2026-08-11: `bevy_matchbox` 0.14
  still pins `bevy ^0.18` while this game is on 0.19, and it fails quietly,
  since a 0.19 app that adds it *resolves*, ending up with both versions in
  the lockfile and a plugin built from types the 0.19 `App` will not take.
  `bevy_ggrs` 0.22 does take `bevy ^0.19`.

- **Rising Tide, a flag-gated round variant.** Rows flood progressively
  during the round, washing away the posts standing on them and becoming
  impassable, so the board the players finish on is smaller than the one
  they started on. Cut from the spec (§3.7) rather than built, because it
  competes with the gulls for the same attention: both are pressure that
  arrives on a timer and invalidates placements. Worth it only if playtest
  shows it makes matches better, not merely busier.

  Not to be confused with the tide events already in `board/events.rs`,
  which are the sparkling crab's roulette and are built. Nothing in the
  tree floods a row.

- **A right-stick flick that places without the cursor.** Input is a
  two-stage model throughout, move the cursor then commit a direction (spec
  §8). A flick of the right stick would collapse both stages into one
  gesture, which is the expert's version of the same verb and much faster
  under pressure. Never prototyped, and no right-stick handling exists at
  all. The question is whether it can coexist with the cursor rather than
  replace it, since every other seat, the keyboard included, keeps the two
  stages.

- **The round-end spectacle.** A wave that came in over the whole beach at
  zero shipped and was taken out again (2026-10-16): drawn as a tinted sheet
  over the board, it looked bad, and the results card hid most of it. The
  round now ends on the horn and the card. The other half of the original's
  ending did not ship either, the sheltered crabs
  scuttling home to sea. The sim is frozen at the wave (spec §3.7) and the
  crabs it holds are the *scored* ones, so sending them anywhere is a
  render-side fiction that has to invent both a count and a route. Not worth
  unfreezing anything for.

## Steam and the Steam Deck

A project rather than a list, so it keeps its own section. Two things it
wanted were owed to desktop players too and are done: borderless
fullscreen by default (2026-09-28, F11 or a settings row to leave it), and
the small-text floor, raised on 2026-09-25 (fine print 15px, card copy
17px).

The shape of it, checked 2026-08-17 and re-checked 2026-09-17: the game is
in good order for a controller-first platform, `gamepad::pad_menu_bridge`
already mirroring the d-pad onto W/S/A/D and South onto Enter, and the
seat-claiming ceremony already running off Start. What is missing is *text*,
both the text a player has to type and the text the game uses to name keys
it no longer has. Those two are the work; the rest is paperwork and
packaging.

Valve grades a Deck build Verified / Playable / Unsupported on four counts:
input, display, seamlessness, system support. Seamlessness and system
support are already met, one binary, no launcher, no dependency prompts, no
anti-cheat, so what follows is input and display.

- **Text entry needs a keyboard nobody has.** The lobby refuses an empty
  name (`lobby_needs_name`), so on a Deck there is no way to host or join
  at all without summoning Steam's on-screen keyboard by hand, which is
  itself the difference between Verified and Playable: the game is expected
  to invoke it. The same holds for join-by-address, chat (`T`), the
  editor's `F1` naming, and seat names on the match-setup card.

  Two ways out. Link `steamworks-rs` and call
  `ShowFloatingGamepadTextInput` when a naming row opens, which is the
  honest fix and the same dependency the achievements below want. Or make
  typing optional, with a pad-driven letter grid or a default name the lobby
  accepts. The second is cheaper and works off Steam too.

- **Every prompt line names the device in hand.** Done 2026-10-22: one
  table per face of the game (`gamepad::scheme`) says what each pad control
  does, the bridge presses the keys it names, the screens that read a pad
  themselves ask it, and the prompt is written from it whenever a pad was
  touched last (`LastDevice`). Every keyboard action has a pad button,
  the editor included; quitting from the menu stays the keyboard's and the
  pause card's, and text entry stays the keyboard's (the prompt says
  "Steam + X: keyboard").

- **Turn the update check off under Steam.** It defaults on
  (`check_updates: true`) and offers to hand a GitHub release page to the
  browser. Steam ships its own updates, and a store build pointing players
  at downloads outside the store is at best confusing to them and at worst
  a review comment. The mechanism already exists, `PINCH_NO_UPDATE`, so
  this is a default, keyed on `SteamAppId` being in the environment or on a
  `steam` cargo feature.

- **Steam Cloud is nearly free here.** Saves live in
  `~/.config/pinch-points` and `~/.local/share/pinch-points` (`app::paths`),
  which is exactly what Auto-Cloud's `LinuxHome` root wants. Worth doing on
  the day: a Deck and a desktop then share a campaign, which is the thing
  players notice.

- **Achievements are ours, not Steam's.** The game keeps 48 of its own
  (`ACHIEVEMENTS`), and a player who sees an achievements screen on a Steam
  game expects them on their profile. That is `steamworks-rs` and a hook in
  `achievements::track::unlock_new`, the same dependency the on-screen
  keyboard wants, so both at once or neither.

- **Packaging.** Build inside the Steam Linux Runtime 3.0 (sniper)
  container and set it as the depot's compat tool: building on a host glibc
  works on SteamOS, which is current, and breaks on older desktops. Add
  Bevy's `wayland` feature beside the default `x11` for desktop Linux; the
  Deck runs Xwayland under gamescope, so x11 alone would do there.
  `system_clipboard` (the share codes) goes through X11 and wants testing
  under gamescope; it already degrades politely (`code_copy_failed`). Ship
  `assets/` beside the binary, with both font licences, which are already
  in `assets/fonts/` and which the OFL requires travel with the font.
  Consider capping to 60 to spare the battery.

- **The paperwork.** Steam Direct is $100 per app, refundable at $1,000 of
  sales, and the tax and bank forms come before the release button. The
  store page wants capsules at 616x353, 460x215, 231x87, a 1920x620 hero
  and 600x900 library art, a trailer, and screenshots; `docs/screenshots`
  is a head start at five. Then the content survey and age rating, a build
  pushed through SteamPipe with the Linux launch options set, and finally
  the Deck compatibility review, which is requested from the partner site
  once a build is up and comes back with the specific failures named.

- **How to test without owning one.** Run the release build under gamescope
  at Deck resolution, `gamescope -W 1280 -H 800 -f --
  ./target/release/pinch-points`, and play it through with only a
  controller in your hands: menu, match setup, a round, the lobby. Every
  point where the keyboard comes back is a finding, in the order Valve will
  find them.

## Tried, and it lost

Kept because the next person to have the idea deserves the measurements.

- **Stop a solver run at a repeated board state.** The idea: the sim is
  deterministic, so a board that recurs will recur for ever, and a run
  could stop the moment one does rather than grinding every dead end out to
  the 1800-tick limit.

  Built 2026-08-14 and reverted. It is *correct*, validating all 38 levels
  that shipped at the time identically with the whole suite passing, and it
  is *slower*. The `--ignored` level check went 120 s to 147 s, and two
  earlier shapes were far worse (332 s hashing the whole board per tick;
  still losing when cut down to only the parts that move). The arithmetic is
  the reason: watching costs a hash and a set insert every tick of every
  node, while the runs that cycle are a small minority, 51 of roughly 2700
  nodes on the one test measured. Skipping the watch until a run has passed
  150 ticks recovered most of the loss and still did not pay for itself.

  Both cautions found earlier hold and are worth keeping: the transition
  reads the absolute tick (spawner periods, gull period), so the key needs
  `tick % lcm(periods)` rather than the tick itself; signposts under
  `CapPolicy::Evict` age against the absolute tick, so a level under that
  rule cannot use the shortcut at all; and a survive-goal level that cycles has
  *won*, not lost. Also: the PRNG state is hashed, and it advances whenever
  a spawner or a gull consumes it, so on a busy board no two moments ever
  match and the shortcut is pure overhead.

  If it is attempted again, the thing to attack is not detection cost but
  the search itself: placing A then B reaches the same board as B then A,
  and both subtrees are explored today. Transposition pruning would cut
  whole branches rather than tail ticks, but it changes which solution is
  found first, so `every_campaign_level_is_solvable_with_its_solution` and
  `granted_signposts_are_necessary` are the guards to watch.

- **Challenges (Beach Day).** Eight 30-second score-attack stages under
  versus arrow rules, with bronze, silver and gold medals. Felt like
  puzzles with a timer; removed 2026-10-23.
