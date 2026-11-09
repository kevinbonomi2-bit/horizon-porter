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

## Next

1. **The rest of block entities:** renderers (`block_entity_submit`), screens (`screen`,
   `screen_draw`) and shift-clicks (`menu_quick_move`); tanks, energy and hopper transfers.
2. **Placed and broken by a player** (`onPlace`/`setPlacedBy`, `playerWillDestroy`): global
   hooks on `Hooks`, filtered to the mod's blocks.
3. **The rest of `World`**: giving and dropping items, the hand, particles, tags and schedules on
   a click (a tick has them).
4. **The mod's own hooks** (events and Mixins the bundle stubs): as `Hooks` methods.
5. **`--api` in the app**, and a build that installs `api/` with the fork's launcher.
