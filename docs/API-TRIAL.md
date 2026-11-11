# Trial: a port as a trait mod on Horizon's new mod API

On 2026-11-09 Horizon's team announced that native mods move from horizon-mixins bundles (the game
rebuilt per mod) to a typed mod API: a mod implements `Hooks`, attaches Parts (`BlockPart`,
`ItemPart`, `BlockEntityPart`, `MenuPart`: one per Java `Block`/`Item`/`BlockEntity`/`Menu`
override), and compiles in about a second against a daily prebuilt game. The work is in the fork
[arismoko/horizon](https://github.com/arismoko/horizon) (its board, direction issue #771 and the
modding book at arismoko.github.io/horizon). This trial took one port, Macaw's Fences and Walls,
and made it a trait mod by hand, to see what of horizon-porter's output carries over.

## What was built

`ai-test/api-trial/mcwfences/` (local, not committed):

- `horizon-mod.toml`, `Cargo.toml`, `assets/`, `data/`: the port's content mod, with `[native]`
  pins to the prebuilt game of 2026-11-09 (Horizon `62c91f4`).
- `src/runtime.rs`: the port's module, **unchanged**: 1,245 lines, its placement, gate and
  fence-shape functions as translated from the Java.
- `src/lib.rs`: 311 lines of glue in place of the mixin bundle's helpers and
  `horizon-mixin.toml`. One `BlockPart` (`state_for_placement`, `update_shape`, `use_item_on`,
  `use_without_item`) attached to the 32 blocks with code; it turns the API's events into the
  porter's `Placement`, `ShapeUpdate` and `BlockUse`, and the porter's `World` into level writes
  through the server host. States go by their full names (`block_state_string`,
  `block_state_id`), so no game internals are reached.

It compiles against the API with no warnings, and the fork's launcher built it on the prebuilt
route in **39 seconds** (`native-build --profile optimized`: "Prebuilt game: only the mods
compile"), against about 5 minutes for a mixin build.

## What carries over

- **The translated code, all of it.** `runtime.rs` talks to the porter's own traits (`World`,
  `Blocks`, `Machine`), never to Horizon, so it moved unchanged. Only the glue is new, and it is
  per kind of behaviour, not per mod: the porter can write it as it writes the mixin helpers
  today.
- **The content mod.** Blocks, items, models, recipes and tags are the same files.
- **The Kinds map onto Parts one to one.** Every behaviour the porter ports has a Part method in
  the API's catalog: placement, use with and without an item, shape updates, random and scheduled
  ticks, survival, comparators, removal and fall/inside on `BlockPart`; use, use on a block,
  tooltips and inventory ticks on `ItemPart`; ticks, menus, shift-click, screens and drawing on
  `BlockEntityPart`; slot rules on `MenuPart`.
- **Several of our Horizon requests are already answered by the API:** tag checks and light on the
  level (`block_in_tag`, `brightness`: request #5), a block state by name (`block_state_id`:
  #4), slot rules a mod's code can answer (`MenuPart::menu_slot_rules`: #14), and an
  `entity_inside` part method (#13).

## In game

The prebuilt game crashed at start here (below), so the game was built from source at the same
commit instead (`native-build --profile balanced`: 944 s the first time; later builds recompile
only the mod). In a creative world, with `/modprofile` counting the mod's calls:

- A player placing a spruce curved gate ran the ported `getStateForPlacement` through the part's
  `state_for_placement`: it faced east with its hinge on the right, as the Java picks them.
- Clicking an oak curved gate ran the ported `useWithoutItem` through `use_item_on` (0.09 ms): it
  opened, and closed on the next click.
- Placing blocks beside a metal fence ran its ported `updateShape` through `update_shape`
  (0.01 to 0.04 ms a call).

Two more stand-ins were needed for this game: the fork's own `horizon-prepare` (its newer
resource format), and the vanilla fluid states table that the fork's newer Java exporters write
(`fluid_state_definitions`), added to `block-behaviours.json` by hand.

## What didn't work yet

- **No playable fork release to build on.** The fork publishes prebuilt game libraries
  (`prebuilt-<commit>`), not releases or Nightlies, and its launcher lists none. The trial named
  a copy of an installed Nightly `nightly-2026-11-09-62c91f4` so the launcher took it as a base at
  the prebuilt's commit.
- **The prebuilt game crashes at start on this Windows machine** (`0xc0000005`, before any log),
  with an empty mod as well as with ours. Stack size is
  the same as Horizon's own builds (8 MB). Building the whole game from source at that commit
  got around it (above).
- **The API moves daily.** Between the prebuilt (`62c91f4`) and the next day's code, three names
  the glue uses lost their `Native` prefix (`NativeBlockClick` → `BlockClick`...). A port pins one
  prebuilt; the porter would regenerate its glue for the API it targets.
- **Small gaps in the glue:** a click's level random isn't reached (the glue draws its own), and a
  custom dimension isn't named from `LevelDimension::Custom`.

## What it means for the porter

The porter can target the API by adding a backend that writes a trait mod (`horizon-mod.toml`
with `[native]`, `Cargo.toml`, `src/lib.rs` glue, the same `runtime.rs`) beside the mixin bundle
it writes today, and keep the bundle while players' launchers run upstream Horizon. Tier 2's
helpers belong in that glue, on the API's `ServerHost` and handles, not on game internals.

## Generated: `port --api`

The glue is now written by the porter (`crates/porter-translate/src/api.rs`): `port --api` puts
the mod in `api/` beside the bundle. The 7 trial mods with code (Farmer's Delight, Villagers
Plus, Advanced Netherite, Beautify, Friends & Foes, Naturalist, Macaw's) all compile against the
pinned API with no warnings, and the generated Macaw's mod plays as the hand-written trial did:
`/modprofile` counted its `PortedBlock` placing a birch curved gate (facing south), opening it on a
click, and reshaping a metal fence.

## Block entities

`port --api` now covers block entities too: each type declared with its blocks and a
`BlockEntityPart`, its saved state from runtime.rs's defaults, its tick on runtime.rs's `Machine`
(its slots, data slots, the blocks beside it and their saved values, recipes, tags, item facts,
sounds, particles and drops), its container (`block_entity_container`) and what clients get of it.
A click's or an item's code reaches block entities through the API's handles, and can give and
drop items, change the hand's stack, wear a tool, send particles and test tags.

Checked in the fork's game with Farmer's Delight: its cooking pot cooks beef stew on a campfire and
keeps it for a bowl, and its cutting board takes a cabbage that a knife cuts into two leaves.

Two things the API didn't give as expected: a mod's own recipe type comes with empty ingredients
and results (the glue reads them from the recipe's JSON, as the bundle does), and the update tag a
client gets has no host to name items with (the glue keeps the ids its ticks and clicks named).
A player's hands go through `LivingRef::new`, a game-side helper.

## Renderers, screens and shift-clicks

A block entity draws through `block_entity_submit`: runtime.rs's renderer gets the items the update
tag carries and the block's facing, and its pushes, turns, cubes, blocks and items go to the API's
draw context. A machine's menu lays its slots out where the Java menu does (`screen`) and draws the
mod's texture and progress sprites under them (`screen_draw`). A shift-click runs the Java menu's
`quickMoveStack` (`menu_quick_move`), except when it moves the whole stack: the API's stacks are
never empty, so that one goes the game's way (Horizon request 15). What a client gets of a block entity names its
items through the API's own item table (`__private::item_key_id`, a game-side helper), since that
hook has no host. Items a block drops get Java's
pickup delay, so they pop out before the player picks them up.

## Clicks, drops and tools

Horizon's client can't run a mod block's `use_item_on` (it is server-only), so it sends the off
hand's use of a click the main hand already took; Java's client never does. A cutting board read
that as an empty hand and took back what it had just been given. The glue answers the off hand's
follow-up as the main hand did (Horizon request 16). Broken, a block entity that isn't a container
drops what it holds (`pre_remove_side_effects`), as Java's `preRemoveSideEffects` does. A tool
whose settings spell its material out (`.durability(FLINT.durability()).attributes(..)`, Farmer's
Delight's knives) gets its durability, a stack of one, its attack, repair item, enchantability and
mining rules. Translation is told that a slot holds at most the item's own stack size.

Checked in survival: a knife lies on an empty board and comes back to an empty hand, a cabbage
stack stays on the board, each cut takes one, and a broken board drops what was on it.

## Baskets: dropped items, redstone and vanilla menus

A Farmer's Delight basket is a 27-slot chest-menu container whose tick pulls in the dropped items at
its open side, paused by redstone through `neighborChanged`. Four things were missing and are now
in the porter: a block entity behind a vanilla menu gets that menu's slots and a template that opens
it (a barrel, which faces six ways as a basket does); a boolean state property Java declares and
doesn't set starts `true`, as `StateDefinition.any()` gives it; a tick can read the dropped items in
a box and take from them (`machine.dropped_in`, `machine.take_dropped`, through the API's
`entities_in`, `item_entity` and `discard`), read its power and its block's state; and
`neighborChanged` is ported: Horizon's `neighbor_changed` part can't change the world, so the glue
asks for a block tick and runs the Java's code in it. Checked in game: a basket pulls in what lands
at its opening and nothing beside it, a hopper under it drains it, a powered lever pauses it, a
comparator reads it, and a basket beside a cutting board catches the cut pieces.

## A fresh port gets the same

To check that these results come from the porter and not from hand work on one port, Farmer's
Delight was ported again into an empty folder with `port --api` and translated from scratch on
Claude Haiku 5.5 (117 functions, all ported; about $1.85). Built and played on its own, it did
what the trial port does: a knife lies on an empty board and comes back to an empty hand, a
cabbage stack stays on the board and each cut takes one, a broken board drops what was on it, the
pot cooks beef stew into a bowl, the skillet cooks one beef every few seconds, and a feast serves
into a bowl. Neither translation ports the stove's own six-slot grill yet (it heats what's on it,
but doesn't cook food put straight on it), and a whole-stack shift-click goes the game's way
(Horizon request 15).

## Refreshed, and the same on today's Horizon (0.9.1)

`translate --refresh` ported 89 of the trial port's functions again (about $1.90 on Haiku) against
what runtime.rs gives now: tag, water and light queries, dropped items, redstone and state in a
tick. Rebuilt on the mod API and played: the cutting board, knife, basket, cooking pot, skillet and
feasts behave as before, seeds plant, and rice plants into water and keeps it. The second, fresh
port was built the way the app builds it, on Horizon 2ebf00a, and played the same; getting there
found that Horizon's sound and particle helpers had changed (fixed in the porter) and that
comparators didn't read a mod's containers (the port now counts the slots itself).

## Next

1. **The rest of block entities:** tanks, energy and hopper transfers.
2. **Placed and broken by a player** (`onPlace`/`setPlacedBy`, `playerWillDestroy`): global
   hooks on `Hooks`, filtered to the mod's blocks.
3. **The rest of `World`**: giving and dropping items, the hand, particles, tags and schedules on
   a click (a tick has them).
4. **The mod's own hooks** (events and Mixins the bundle stubs): as `Hooks` methods.
5. **`--api` in the app**, and a build that installs `api/` with the fork's launcher.

## Catching up with the fork (2026-11-10)

The fork's tracker ([playsunrise.net/api-gaps](https://playsunrise.net/api-gaps)) lists 38
capabilities covered, 32 partial and 5 missing against NeoForge, and the API itself moved from
`horizon-mod-api` into `horizon-mod-vocab` (one file a hook: 637 hooks, 37 host files). The glue
`port --api` writes is still pinned to `62c91f4`. Type-checked against the fork's main
(`c7c49ff`), Farmer's Delight's glue needs seven edits and nothing else:

- `NativeBlockClick`, `NativeBlockUpdateFlags`, `NativeInteractionResult`, `NativeBlockFace`,
  `NativeSupportType`, `NativeItemDisplayContext` and `NativeScreen` lost their `Native`.
- `Screen` has two more fields (`recipe_book`, `recipe_book_toggle`): build it with
  `..Default::default()`.
- `level_tick_post` takes a `HostSide` and its event a `LevelRef` (`event.level.dimension_id()`).
- `LevelRef::game_event` takes a `GameEventContext`.

What the fork's API has that ported code still asks for, by the note ports leave:

| Ports say they miss | In the fork's API |
| --- | --- |
| Mobs near a block (`getEntitiesOfClass`) | `level_entities_in`, `mob`, `living_baby`, `mob_set_baby` |
| A grave's "player died, here are the drops" | the `living_drops` hook (drops can be taken), `player_clone`, `player_died` |
| Villager trades for a trading post | `merchant`, `merchant_offers`, `player_open_menu` |
| Status effects on a player or mob | `living_add_effect`, `living_remove_effect` |
| Stats and item cooldowns | `player_award_stat`, `player_set_item_cooldown` |
| A ray from the player's eyes (`getPlayerPOVHitResult`) | `level_clip`, `entity_pick` |
| Whether the player can eat | `player_can_eat`, `player_food` |
| Item particles (`ItemParticleOption`) | `item_particle_options` |
| A mod's own screens and widgets | typed widgets, menu Parts, `screen_draw`, GUI layers |

Still open there too: general mod entities (#792: mobs are copies of a vanilla one), non-mob
entity renderers (#860), typed network payloads (#813).

Regular Horizon has none of the left column for hook code except `living_death` and
`player_died`, which only listen. So these come to ports through `--api` first.

## On the fork's Nightly (2026-11-10)

The fork publishes Nightlies now, and `port --api` targets one: `34724f7` (Nightly 2026-11-10).
The seven edits above are in `api.rs`, and all twelve trial mods' glue type-checks against that
commit's `horizon-mod-api`.

How it was set up here, with nothing done by hand to the game this time:

1. The fork's launcher, built from the Nightly's own source (`launcher/`, `cargo build
   --release`): the notes say to install with it, and there is no prebuilt fork launcher.
2. `horizon-launcher install nightly` (it downloads and verifies the build), in a launcher folder
   of its own (`HORIZON_LAUNCHER_HOME`), with the Rust toolchain copied from the older one.
3. `horizon-launcher prepare --version nightly-2026-11-10-34724f7 --minecraft <folder>`, the
   folder being the regular Horizon launcher's own Minecraft (`horizon-launcher/minecraft`).
4. `horizon-launcher native-build <port>/api --instance "api nightly" --version
   nightly-2026-11-10-34724f7 --trust-native-code`. There is no prebuilt game for a Nightly's
   commit (only for the daily `prebuilt-<commit>` ones), so the first build compiles the game.
5. The game started on its own from inside the launcher's folder stops with "start the launcher
   once to register this installation": it wants leases the launcher's window sets up. For
   scripted checks, a copy of the version folder outside the launcher's folder runs without them.

What ported code can reach through it now, beside what it had: `World::stat` (`awardStat`),
`cooldown` (`addCooldown`), `effect` (`addEffect` on the player), `can_eat`, `ray` (`level.clip`,
`getPlayerPOVHitResult`), `mobs_in` (`getEntitiesOfClass`) and `mob_love` (`setInLove`). On
regular Horizon these do nothing, as its hooks give no way to.

A block entity's tick has the mob query too (`machine.mobs_in`, `machine.love`).

The glue also runs, on this API, what it left to horizon-mixins bundles before:

- **A block placed or broken by a player** (`setPlacedBy`, `playerWillDestroy`): `break_block`
  runs the port's `broken` before the block goes; `entity_place` tells of a placement before the
  block is written, so the port's `placed` runs from a task a tick later, once it is there.
- **A dying player's drops** (`LivingDropsEvent`): the classes that hear it, and those that hear an
  event of the mod's own posted from there, become one function, `on_player_drops`, called from
  `living_drops`; true takes the drops. A block entity that keeps its items inside a class it
  holds (GraveStone's `Death`) gets item slots of its own for them, so the function can put the
  stacks in the grave and breaking it gives them back.

Still to give: trades and menus for a player, a mod's own screens as widgets, mob goals.
