# What the census says to fix next

Read from [CENSUS.md](CENSUS.md) (150 mods, 2026-11-08, `tools/census.py`), with each
gap checked against Horizon's hook catalog (`native-hooks.json`, 432 hooks). Counts
are mods, out of 150. A fix's reach is its count; the tiers put first what Horizon
already supports, so the porter alone can close it.

## Tier 1: Horizon has the hook, the porter doesn't wire it

**Done 2026-11-08** (commits 841e17f..): every row but `neighborChanged` is wired, with a
stub to translate, and blocks with `randomTick` or `.randomTicks()` tick randomly. In the
150-mod census the ports now name `randomTick`, `canSurvive`, `setPlacedBy` and
`playerWillDestroy` in every mod that has them, tooltips in 13 of 15, comparators in 6
of 10, and `quickMoveStack` in 7 of 16: the rest are menus with no block (backpacks,
anvils, Curios), which aren't machines. Two hooks can't take ported code (requests
#13 and #14 in the Horizon request): `stepOn`/`entityInside` (static) and slot rules
(in `horizon-client-lib`). Checked in game: Farmer's Delight's compost advances on
random ticks, and the pot's shift-click moves its input to the player.

The Java method is common, Horizon has a hook point for it, and the porter drops it
today. Each one is the same kind of work as `useItemOn` → `right_click_block`: find
the method, write a stub in runtime.rs, wire the hook, give the translation a `World`.

| Java method | Mods | Horizon hook |
| --- | --- | --- |
| Menu `quickMoveStack` (shift-click), `mayPickup`, `mayPlace`, `canPlaceItem`, `onTake` | 16 | `menu_quick_move`, `menu_slot_rules` |
| Item / block `appendHoverText` (tooltips) | 15 | `tooltip` |
| Block `onPlace`, `setPlacedBy` | 11 | `place_block` |
| Block `playerWillDestroy`, `playerDestroy`, `affectNeighborsAfterRemoval` | 11 | `destroy_block` |
| Block `get/hasAnalogOutputSignal` (comparators) | 10 | `has_analog_output`, `analog_output` |
| Block `entityInside`, `stepOn`, `fallOn` | 9 | `block_entity_inside`, `block_step_on`, `block_fall_on` |
| Block `randomTick` (and `.randomTicks()` in its settings: 5) | 7 | `random_tick` |
| Block `canSurvive` | 7 | `can_survive` |
| Block `neighborChanged` | 7 | `neighbor_changed` (listen only; request #3 asks for a writable one) |

## Tier 2: helpers ported code needs

Calls the mods' game classes make that `World` / `Machine` have no helper for. Each
is a method on the trait plus its Horizon call, like `remainder` and `particles`.

| Java call | Mods | What it does | Horizon has |
| --- | --- | --- | --- |
| `level.scheduleTick(pos, block, delay)` | 14 | a block's own tick later | `native_schedule_block_tick` / `_fluid_tick`, and the `tick_block` hook runs it |
| `level.levelEvent(..)` | 13 | vanilla's sound-and-particle events (door, smoke, break) | sounds and particles separately |
| `level.destroyBlock(pos, drop)` | 12 | break with drops and particles | `native_set_block` + `native_summon` |
| `level.getEntitiesOfClass(..)` | 11 | the mobs or items in a box | to check |
| `player.awardStat(..)` | 11 | statistics | to check (`player_stat_award` hook listens) |
| `level.gameEvent(..)` | 10 | sculk vibrations | `native_game_event` |
| `entity.setDeltaMovement(..)` | 10 | push, launch, bounce | to check |
| `player.sendOverlayMessage(..)` | 9 | the action-bar line | `native_action_bar` |
| `entity.hurt(level.damageSources()..)` | 9 | damage | to check (request #12 asks for health) |

## Tier 3: block and item settings the content mod ignores

Read from the registration as `.strength(..)` is; most map to a block or item file
field or component.

| Setting | Mods | In Horizon's content |
| --- | --- | --- |
| `.mapColor(..)` | 12 | the block's map color |
| `.pushReaction(..)` | 8 | how pistons treat it |
| `.instabreak()` | 5 | hardness 0 |
| `.randomTicks()` | 5 | random ticks (with Tier 1's `random_tick`) |
| `.ignitedByLava()` | 5 | lava sets it alight |
| `.noCollision()` | 4 | no collision shape |
| `.jukeboxPlayable(..)` | 3 | `minecraft:jukebox_playable` |
| `.replaceable()`, `.cookingFuel(..)` | 2 each | replaceable; `minecraft:cooking_fuel` |

## Tier 4: block entity state

`NonNullList<ItemStack>` fields (7 mods) are the block entity's items: map them to its
slots, as the cooking pot's `ItemStackHandler` already is. `Component` (4, a custom
name) and `UUID` (2, an owner) can be kept as text.

## Later: needs Horizon or is a big area of its own

- **Mobs' AI and data** (`registerGoals`, `Goal.canUse/start/tick`,
  `defineSynchedData`, save data: 6 to 9 mods): content mobs copy a vanilla template
  (request #10).
- **Config and other screens** (`init`, `extractRenderState`, widgets: 35 to 45
  mods): mostly config screens. Horizon's native screens exist; porting widget
  trees is its own project.
- **World generation** (`findGenerationPoint`, features, structures: 6+): request #1.

## Not gaps (reached without the porter naming them)

The census can't see these as handled, but they are: `getShape`,
`getCollisionShape`, `createBlockStateDefinition` (read into the block's shapes and
states), `getRenderShape`, `newBlockEntity`, `getTicker` (block entities are linked
and ticked), `rotate` and `mirror` (the block's template), `saveAdditional` /
`loadAdditional` (the block entity state struct), `getUpdateTag`.

## Cost of translating the corpus

`translate --dry-run` over the 113 mods with code: 1,488 functions, about $4.55 with
Haiku for one try each (retries and fixes add some). Translating every surveyed mod
after Tiers 1 and 2 is affordable, and the translations' own "Missing:" notes then
become the next census.

## What ports still say is missing (0.9.1)

After `translate --refresh`, Farmer's Delight's ported code still carries 96 `// Missing:` notes.
By theme, the next things to give ported code:

| Missing | Notes | Would need |
| --- | --- | --- |
| Block shapes and sturdy faces (`canSupportCenter`, `isFaceSturdy`, collision tests) | 12 | a shape query on `Blocks` |
| Light and sky where the level hook gives none (`canSeeSky`, light in `canSurvive`) | 10 | Horizon request 18 |
| Entities: damage, effects, passengers, the placer | 10 | Horizon requests 9, 12, 13 |
| Stats and advancements (`awardStat`, criteria triggers) | 8 | a `World::stat`, to check in Horizon |
| The mod's own config holders (not `ModConfigSpec`) | 7 | reading Cloth/owo/custom config classes |
| Item and block particles (`ItemParticleOption`, `BlockParticleOption`) | 4 | particles with a payload |
| Rolled results (`rollResults`, chance outputs, Fortune) | 3 | recipe results with chances |
| Overlay messages (`displayClientMessage(.., true)`) | 2 | `World::overlay` (`native_action_bar`) |

Also: a block entity whose slot count comes from a method (`getInventorySlotCount()`, Farmer's
Delight's stove) gets no slots, so its own cooking isn't ported; `level.destroyBlock`,
`levelEvent` and `gameEvent` from Tier 2 are still open.
