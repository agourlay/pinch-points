# Backlog

Remaining ideas and known follow-ups, roughly ordered by value. Done work
lives in git history, not here, including the measurements behind it, which
belong next to the code they explain.

## Gameplay and modes

- **Stop a solver run at a repeated board state: tried, and it loses.**
  The idea: the sim is deterministic, so a board that recurs will recur for
  ever, and a run could stop the moment one does rather than grinding every
  dead end out to the 1800-tick limit.

  Built 2026-08-14 and reverted. It is *correct* (all 38 shipped levels
  validated identically and the whole suite passed) and it is *slower*.
  The `--ignored` level check went 120 s → 147 s, and two earlier shapes of
  it were far worse (332 s hashing the whole board per tick; still losing
  when cut down to only the parts that move). The reason is arithmetic, not
  soundness: watching costs a hash and a set insert every tick of every
  node, while the runs that actually cycle are a small minority: 51 of
  roughly 2700 nodes on the one test measured. Skipping the watch until a
  run has passed 150 ticks recovered most of the loss and still did not pay
  for itself.

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

- **The round-end spectacle, half of it.** The wave shipped
  (`board_render::wash`): at zero the sea comes in over the whole beach,
  holds, and drains, with everything the round built underneath it. What
  did not ship is the other half of the original's ending, the sheltered
  crabs scuttling home to sea. The obstacle is that the sim is frozen at
  the wave (spec §3.7) and the crabs it is holding are the *scored* ones,
  so sending them anywhere is a render-side fiction that has to invent
  both a count and a route. Worth doing when someone wants it; not worth
  unfreezing anything for.

- **Rising Tide, a flag-gated round variant.** Rows flood progressively
  during the round, washing away the posts standing on them and becoming
  impassable, so the board the players finish on is smaller than the one
  they started on. Cut from the spec (§3.7) rather than built, because it
  competes with the gulls for the same attention: both are pressure that
  arrives on a timer and invalidates placements. Worth it only if playtest
  shows it makes matches better, not merely busier.

- **A right-stick flick that places without the cursor.** Input is a
  two-stage model throughout, move the cursor then commit a direction (spec
  §8). A flick of the right stick would collapse both stages into one
  gesture, which is the expert's version of the same verb and much faster
  under pressure. Never prototyped. The question is whether it can coexist
  with the cursor rather than replace it, since every other seat, the
  keyboard included, keeps the two stages.

## Netcode

Versions checked 2026-08-11. Worth keeping straight before reading either
entry: `ggrs` and `matchbox_socket` are engine-free, carrying no Bevy
dependency at all, the property the sim insists on for itself, while
`bevy_ggrs` and `bevy_matchbox` are the wrappers that marry them to the
ECS. It is the wrappers that lag Bevy releases, and the wrappers this game
has least use for.

- **Online lobby beyond LAN** (matchbox-style signalling), if the game ever
  leaves the living room. `transport.rs` speaks UDP to a `SocketAddr` given
  by hand, which is a LAN and nothing else. `bevy_matchbox` 0.14 does still
  pin `bevy ^0.18`, and it fails quietly rather than loudly: a 0.19 app that
  adds it *resolves*, ending up with both 0.18 and 0.19 in the lockfile and
  a plugin built from types the 0.19 `App` will not take. Reach past it to
  `matchbox_socket`, which is engine-free and has an optional `ggrs`
  feature: the two pair without a Bevy wrapper in the middle.

  **Rollback, on `ggrs` directly**, filed here because this is what makes
  it worth having, not the other way round. Nothing blocks it today:
  `bevy_ggrs` 0.22 already takes `bevy ^0.19` and plain `ggrs` 0.13 depends
  on no engine. What is missing is the need. Rollback's whole product is
  giving back the input delay, and that delay is 100 ms (three frames at
  30 Hz), online only (couch play ticks the sim directly) against a
  common crab that takes 710 ms to cross a tile. A seventh of a tile, on
  the mode fewer people play, over a link whose round trip is a
  millisecond. At 40-80 ms of real internet the delay has to grow to four
  or six frames and the waiting stops being rare; that is when prediction
  earns its keep.

  Skip `bevy_ggrs` whenever it does happen. It exists to roll back ECS
  component state, and the sim is deliberately outside the ECS (spec §7.2)
  an integration layer for a problem this game does not have. Raw `ggrs`
  fits what is here already: `Config::State` is a `Board` (`Clone`,
  `state_hash`, `to_snapshot`), `Input` is the 3-byte packed action,
  `Address` is a `SocketAddr`, and `transport.rs` is the socket. One more
  piece fell into place by accident: `sim_events.rs` diffs the board once a
  frame rather than hooking the moments things happen, so a re-simulated
  frame emits the net change instead of firing every bank sound twice.
  That is the part that usually makes a rollback retrofit miserable.

  Price: `ggrs` wants serde on the input type, and brings serde, bincode,
  parking_lot and rand with it. All of that lands in the shell beside the
  transport, so `crate::sim` keeps its no-dependency rule.
- **Try a shorter input delay before buying rollback.** `DEFAULT_DELAY` is 3,
  hardcoded at all five call sites, with no adaptivity and no setting. On a
  wired LAN the round trip is about a millisecond, so most of those three
  frames are jitter margin rather than necessity, and delay 1 still leaves a
  33 ms budget. Turning it down recovers two thirds of what rollback offers
  for the price of a constant, and the session waits rather than desyncs,
  so setting it too low trades latency for hitches, which wifi will show
  before ethernet does. Wants two machines and a real LAN to judge.

  Cheaper still, and unrelated to netcode: draw the local player's post the
  moment the key is pressed, greyed until its frame commits. It buys no
  precision, since the post affects nothing until the sim reaches it, but it
  answers "did that register?", which is most of what a player feels.

## Performance

- **Single-threaded executor for the main world.** Now behind the
  `PINCH_ST_EXEC=1` dev hook (`schedule.rs`), which swaps all 46 main-world
  schedules and leaves the render sub-app parallel, so any machine can A/B
  it with the shipped binary. The hook uses `SingleThreadedExecutor::new()`
  and never `::default()`: the derived default leaves
  `apply_final_deferred` false, so queued commands are dropped and the app
  dies on the first frame complaining that a resource does not exist.

  Re-measured 2026-08-12 on the release build (fat LTO), interleaved runs,
  45 s windows, vsync held on both sides. Versus (six seats, XL beach):
  23% less CPU (39.3 s → 30.2 s user+sys) and voluntary context switches
  halved (1.81M to 0.87M; the original 1.78M to 0.89M figure reproduces).
  Menu postcard: 24% less CPU, switches 1.07M → 0.59M, so the win is not
  versus-specific. Wall time and peak RSS identical throughout. The engine
  spends more coordinating this game's hundred small systems than the
  systems spend working.

  Corroborated 2026-08-16 with `perf` on the **debug** build, six seats and
  five AI on the XL beach, 30 s at 499 Hz. The profile is flat: nothing
  reaches 1.3%, and this game's own code is **0.51% of samples**. What is
  left is the engine coordinating itself, `bevy_ecs` at 17.9% and the task
  executor's queues, atomics and mutexes at about 15.9% between them,
  against 4% for wgpu and rendering. Debug numbers do not predict the
  shipped build, but the shape matches the release measurement above, and
  it is the same conclusion from a different angle.

  Also checked while there: no file I/O appears in the profile, which is
  the fix for the map dial reading the shelf off disk every frame holding.

  What is *still* not known is how much frame budget it leaves on a slower
  machine: serializing a hundred systems gives up the parallelism that
  would absorb a heavy frame, and this hardware's uncapped test is not
  trustworthy (an integrated GPU driven at several hundred fps throttles;
  one 22-second run swung between 9 and 615 fps). Before adopting as the
  default, run `PINCH_ST_EXEC=1` on the slowest machine available and
  watch for hitches; the editor and puzzle screens remain unmeasured.

## Steam and the Steam Deck

Checked against the tree on 2026-08-17. The shape of it: the game is in
unusually good order for a controller-first platform, because
`gamepad::pad_menu_bridge` already mirrors the d-pad onto W/S/A/D, South
onto Enter and East onto Escape, and the seat-claiming ceremony already
runs off Start. What is missing is *text* - both the text a player has to
type, and the text the game uses to name keys it no longer has. Those two
are the work; everything under them is paperwork and packaging.

Valve grades a Deck build Verified / Playable / Unsupported on four
counts: input, display, seamlessness, system support. Seamlessness and
system support are already met - one binary, no launcher, no dependency
prompts, no anti-cheat - so what follows is input and display.

- **Text entry needs a keyboard nobody has.** The lobby refuses an empty
  name (`lobby_needs_name`), so on a Deck there is no way to host or join
  at all without summoning Steam's on-screen keyboard by hand, which is
  itself the difference between Verified and Playable: the game is
  expected to invoke it. The same holds for join-by-address, chat (`T`),
  the editor's `F1` naming, and seat names on the match-setup card.

  Two ways out. Link `steamworks-rs` and call
  `ShowFloatingGamepadTextInput` when a naming row opens - the honest fix,
  and the same dependency the achievements below want. Or make typing
  optional: a pad-driven letter grid, or a default name the lobby accepts,
  so the keyboard is a convenience rather than a gate. The second is
  cheaper and works off Steam too, which the first does not.

- **Every prompt line is worded for a keyboard.** `menu_prompt`,
  `prompt_setup`, `prompt_versus_short` and the rest spell out WASD,
  Enter and Esc, and they say it in eight languages. Verified asks that
  on-screen glyphs match the device in the player's hands. The input path
  is already bridged; it is only the words that lie.

  This is the largest single piece of work on the list, because it
  multiplies: either a pad variant of each prompt in every table, or -
  better - glyph substitution at the call site, so the tables carry a
  marker rather than a key name and one place decides what a marker reads
  as. The second shrinks the translation work to nothing and is worth the
  refactor even before Steam. `pad_help1` / `pad_help2` on the settings
  card are the only lines that speak pad today.

- **B quits the game.** The bridge maps East onto Escape, and Escape on
  the menu is `AppExit` (`menu_scene::menu_input`, and the comment there
  explains why the keyboard wants it). So a Deck player pressing B
  expecting "back" leaves the game, with nothing asked. Either drop East
  from the bridge on the menu screen, or put the same two-press
  confirmation on it that the progress reset has.

- **Small text on a seven-inch screen.** The interface is laid out for
  1280x720 and the Deck is 1280x800, so `fit_ratio` stays at 1.0 and
  nothing shrinks - that part is luck, but it is good luck. The problem is
  the absolute sizes: 12.5px, 13px and 15px carry the notes, the hints,
  the menu blurbs and the pad help, and illegible small text is the most
  common Verified failure there is. `UI_SCALE_MAX` is 150, which only
  helps a player who finds the dial. Raise the floor on those sizes, or
  default the UI scale up when the window arrives Deck-sized.

- **It launches windowed.** With no `PINCH_WINDOW` the window is Bevy's
  default 1280x720. Gamescope will scale that to fill the screen, so it is
  not broken, but a first launch should look intended: default to
  borderless fullscreen, with a toggle and a settings row, which desktop
  players have been owed anyway.

- **Turn the update check off under Steam.** It defaults on
  (`check_updates: true`) and offers to hand a GitHub release page to the
  browser. Steam ships its own updates, and a store build pointing players
  at downloads outside the store is at best confusing to them and at worst
  a review comment. The mechanism already exists - `PINCH_NO_UPDATE` - so
  this is a default, keyed on `SteamAppId` being in the environment or on
  a `steam` cargo feature.

- **Steam Cloud is nearly free here.** Saves live in
  `~/.config/pinch-points` and `~/.local/share/pinch-points`
  (`app::paths`), which is exactly what Auto-Cloud's `LinuxHome` root
  wants. Worth doing on the day: a Deck and a desktop then share a
  campaign, which is the thing players notice.

- **Achievements are ours, not Steam's.** The game keeps fifty of its own,
  and a player who sees an achievements screen on a Steam game expects
  them on their profile. That is `steamworks-rs` and a hook in
  `achievements::unlock`, the same dependency the on-screen keyboard
  wants, which is an argument for doing both at once or neither.

- **Packaging.** Build inside the Steam Linux Runtime 3.0 (sniper)
  container and set it as the depot's compat tool: building on a host
  glibc works on SteamOS, which is current, and breaks on older desktops.
  Add Bevy's `wayland` feature beside the default `x11` for desktop Linux;
  the Deck runs Xwayland under gamescope, so x11 alone would do there.
  `system_clipboard` (the share codes) goes through X11 and wants testing
  under gamescope - it already degrades politely (`code_copy_failed`).
  Add `strip = true` to the release profile. Ship `assets/` beside the
  binary, both font licences with it (the OFL requires the notice travel
  with the font), and consider capping to 60 to spare the battery.

- **The paperwork.** Steam Direct is $100 per app, refundable at $1,000 of
  sales, and the tax and bank forms come before the release button. The
  store page wants capsules at 616x353, 460x215, 231x87, a 1920x620 hero
  and 600x900 library art, a trailer, and screenshots - `docs/screenshots`
  is a head start. Then the content survey and age rating, a build pushed
  through SteamPipe with the Linux launch options set, and finally the
  Deck compatibility review, which is requested from the partner site once
  a build is up and comes back with the specific failures named.

- **How to test without owning one.** Run the release build under
  gamescope at Deck resolution - `gamescope -W 1280 -H 800 -f --
  ./target/release/pinch-points` - and play it through with only a
  controller in your hands: menu, match setup, a round, the lobby. Every
  point where the keyboard comes back is a finding, in the order Valve
  will find them.

## Infrastructure

- **An itch.io page**, with the README screenshots. The binaries exist to
  put on it: `v0.1.0` is tagged and `release.yml` builds six targets across
  Linux, Windows and macOS. A Steam build is a longer road and has a section
  of its own above; the update check is the one place the two disagree,
  since it wants to be on for a downloaded build and off for a store one.
- **Balance harness in CI** - run `examples/balance.rs` and
  `examples/ladder.rs` nightly and track seat drift and the difficulty ladder;
  both are cheap - a minute for the pair - and both have caught real
  regressions: an inverted ladder once, and a seat handicap in the bot's
  blunder draw that had been in every round ever played.

  That last one is the argument for the nightly. The figures here were taken
  2026-08-11 and nobody looked again until 2026-08-22, by which time every
  one of them had moved, one from 3.2 sigma to 5.9. Standing at 2026-08-22
  (`BALANCE_FULL=1`), worst seat deviation per generated sweep: two seats
  0.3 sigma, four 1.3 on the 12x9 beach and 1.2 on the 16x11, six 2.5. The
  16x11 run is 200 games against the others' 3000, so its figure is the
  noisy one and not a fifth-column effect; a nightly job should even the
  sample sizes before reading anything into it.

  Read the generated sweeps and not the `classic` ones: those play a single
  handmade board a hundred times with only the warm-up offset varying, which
  is a small and heavily correlated sample, and their sigmas swing several
  points between runs that change nothing they measure.
