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

- **It launches windowed.** With no `PINCH_WINDOW` the window is Bevy's
  default 1280x720. Default to borderless fullscreen instead, with a toggle
  and a settings row. Also a Deck requirement, where gamescope scales a 720p
  window to fill the screen so nothing is broken, but a first launch should
  look intended.

- **Raise the floor on small text.** `type_scale::FINE` is 13px and `BODY`
  15px, and between them they carry the notes, the hints, the menu blurbs
  and the pad help. `UI_SCALE_MAX` is 150, which only helps a player who
  goes looking for the dial. Raise the floor, or default the scale up on a
  small window. Smaller still, and unexamined, are the 11px and 12px in
  `effects.rs`: those are floating score numbers rather than copy, so they
  may be fine, but they are below anything the interface uses deliberately.

  Also a Deck requirement, and the most common Verified failure there is.
  The layout itself is lucky: the interface is built for 1280x720 and the
  Deck is 1280x800, so `fit_ratio` stays at 1.0 and nothing shrinks.

- **An itch.io page**, with the shots in `docs/screenshots`. The binaries
  exist to put on it: `v0.4.0` is tagged and `release.yml` builds six
  targets across Linux, Windows and macOS. A Steam build is a longer road
  and has a section of its own below; the update check is the one place the
  two disagree, wanting to be on for a downloaded build and off for a store
  one.

## Waiting on a machine

Both of these are one-line changes whose whole difficulty is judging them,
and neither can be judged on the machine they were written on.

- **Try a shorter input delay before buying rollback.** `DEFAULT_DELAY` is
  3 (`sim/net.rs:22`), with no adaptivity and no setting; the sessions that
  read it are built in `app/net/rounds.rs` and `app/net/mod.rs`. On a wired
  LAN the round trip is about a millisecond, so most of those three frames
  are jitter margin, and delay 1 still leaves a 33 ms budget. Turning it
  down recovers two thirds of what rollback offers for the price of a
  constant; the session waits rather than desyncs, so too low trades
  latency for hitches, which wifi shows before ethernet does. Wants two
  machines and a real LAN to judge.

- **Single-threaded executor for the main world.** Behind the
  `PINCH_ST_EXEC=1` dev hook (`schedule.rs`), which swaps every main-world
  schedule and leaves the render sub-app parallel, so any machine can A/B
  it against the shipped binary. The hook uses
  `SingleThreadedExecutor::new()` and never `::default()`: the derived
  default leaves `apply_final_deferred` false, so queued commands are
  dropped and the app dies on the first frame complaining that a resource
  does not exist.

  Measured 2026-08-12 on the release build (fat LTO), interleaved runs,
  45 s windows, vsync held on both sides. Versus (six seats, XL beach):
  23% less CPU (39.3 s to 30.2 s user+sys) and voluntary context switches
  halved (1.81M to 0.87M). Menu postcard: 24% less CPU, switches 1.07M to
  0.59M, so the win is not versus-specific. Wall time and peak RSS
  identical throughout.

  Corroborated 2026-08-16 with `perf` on the **debug** build, six seats and
  five AI on the XL beach, 30 s at 499 Hz. The profile is flat: nothing
  reaches 1.3%, and this game's own code is 0.51% of samples. What is left
  is the engine coordinating itself, `bevy_ecs` at 17.9% and the task
  executor's queues, atomics and mutexes at about 15.9% between them,
  against 4% for wgpu and rendering. Debug numbers do not predict the
  shipped build, but the shape matches the release measurement, and it is
  the same conclusion from a different angle. The engine spends more
  coordinating this game's hundred small systems than the systems spend
  working.

  What is *still* not known is how much frame budget it leaves on a slower
  machine: serializing a hundred systems gives up the parallelism that would
  absorb a heavy frame, and this hardware's uncapped test is not trustworthy
  (an integrated GPU driven at several hundred fps throttles; one 22-second
  run swung between 9 and 615 fps). Run `PINCH_ST_EXEC=1` on the slowest
  machine available and watch for hitches before adopting it as the default;
  the editor and puzzle screens remain unmeasured.

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

- **Watching a round that has already started.** A peer the socket picks up
  mid-round is answered with `NetMsg::Queued { ahead }` and waits for the
  next one (`net::rounds::queue_place`). That is deliberate: a lockstep
  session replays from frame zero and the resend tail only reaches
  `resend_span` frames back, 33 at the default delay, so a latecomer
  admitted as a spectator ends up staring at frame zero forever. (Forty was
  the old fixed window, and `sim/net.rs:133` records why it went: it
  deadlocked under a one-way loss burst.)

  Letting them in needs a snapshot join rather than a replay: hand the
  arrival the board as it stands and start their session at that frame
  instead of zero. The format exists and is exercised. The *level* format
  cannot carry a round in progress, which is what the note in
  `tests/it/online.rs` means, but `snapshot-v1` (`Board::to_snapshot` /
  `parse_snapshot`) carries every field `state_hash` covers, and
  `app::suspend` already moves a mid-round board between machines that way.
  What is missing is a `Lockstep` that can begin at a nonzero frame, and a
  wire message to carry the snapshot.

  Worth it on a busy LAN, where someone wandering over to watch is the
  common case and "wait for the next round" is the whole of the answer
  today. Not worth it for a living room where everyone starts together.

- **The round-end spectacle, half of it.** The wave shipped
  (`board_render::wash`): at zero the sea comes in over the whole beach,
  holds, and drains, with everything the round built underneath it. What did
  not ship is the other half of the original's ending, the sheltered crabs
  scuttling home to sea. The sim is frozen at the wave (spec §3.7) and the
  crabs it holds are the *scored* ones, so sending them anywhere is a
  render-side fiction that has to invent both a count and a route. Not worth
  unfreezing anything for.

## Steam and the Steam Deck

A project rather than a list, so it keeps its own section. Two items it
needs are above under **Ready when someone is**, because they are owed to
desktop players too: borderless fullscreen by default and the small-text
floor.

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

- **Every prompt line is worded for a keyboard.** `menu_prompt`,
  `prompt_setup`, `prompt_versus_short` and the rest spell out WASD, Enter,
  Esc, Space and the arrow keys, in eight languages. Verified asks that
  on-screen glyphs match the device in the player's hands. The input path
  is already bridged; it is only the words that lie. Not only a Deck
  problem: a pad player on a desktop reads the same lies, and the one place
  the words and the bridge actually disagreed was a button that quietly
  closed the game.

  The seam to do it at exists and is proven. `KeyCaps::legend`
  (`keycaps/mod.rs:166`) already rewrites key names in a prompt from a
  table of markers, and `hud/text.rs:438` already runs every prompt through
  it, so there is no call-site plumbing to build and no per-language prompt
  variant to write.

  What it does not reach is the hard half. `BLOCKS` matches only spellings
  that are the same in every language: `WASD`, `IJKL`, `W/S`, `A/D`. The
  words a pad player most needs replaced are the localized ones: French
  alone says `Entrée`, `Échap` and `Espace`, and every language spells the
  arrow keys its own way (`touches fléchées`, `矢印キー`,
  `клавиши-стрелки`). Reaching those means the tables carry a
  language-independent marker where they now carry a translated key name, a
  mechanical edit to every prompt line in eight tables. Smaller than writing
  a pad variant of every line, but not a one-liner.

  `pad_help1` / `pad_help2` on the settings card are the only lines that
  speak pad today.

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

- **Achievements are ours, not Steam's.** The game keeps fifty of its own
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
  `CapPolicy::Evict` age against the absolute tick, so a challenge stage
  cannot use the shortcut at all; and a survive-goal level that cycles has
  *won*, not lost. Also: the PRNG state is hashed, and it advances whenever
  a spawner or a gull consumes it, so on a busy board no two moments ever
  match and the shortcut is pure overhead.

  If it is attempted again, the thing to attack is not detection cost but
  the search itself: placing A then B reaches the same board as B then A,
  and both subtrees are explored today. Transposition pruning would cut
  whole branches rather than tail ticks, but it changes which solution is
  found first, so `every_campaign_level_is_solvable_with_its_solution` and
  `granted_signposts_are_necessary` are the guards to watch.
