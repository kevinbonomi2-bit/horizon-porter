# Case study: what porting Create takes

[Create](https://github.com/Creators-of-Create/Create) is the benchmark for
"full mods". This page breaks it into the systems a port must cover, and what each
one needs from horizon-porter, modkit and Horizon itself.

**Licence.** Create's `LICENSE.md` puts its **code under MIT** but its **assets
(textures, models, sounds) under All Rights Reserved**. A port may translate the code,
but it must not redistribute Create's assets: they have to come from the player's own
copy of the mod, the way Horizon's launcher handles modpacks.

**Version.** Create's main branch targets Minecraft 1.21.1; Horizon is 26.3. Its blocks,
block states and models load as they are. Its items needed the item model definitions the game
asks for since 1.21.4, which the porter now writes for a mod that ships none; its 1.21.1
recipes and loot tables weren't checked.

## What the scanner found

`horizon-porter report` on Create 6.0.11 (`mc1.21.1/dev`, 2026-10-28):

| | |
| --- | --- |
| Code | 2,011 classes, 275,038 lines, 16,076 methods |
| Mixins | 65 classes, 144 members: 108 map to a supported horizon-mixins kind, 29 are `@Shadow` (not needed), 5 partial (`TAIL` injections), 2 blocked (Mixins that add an interface) |
| Registries found in code | 570: block 207, block_entity_type 114, item 88, data_component_type 57, and 21 more registries |
| Libraries | Catnip in 997 classes, Flywheel 235, Registrate 93, Ponder 77, ComputerCraft API 45, JEI 32 |
| Loader API | 221 NeoForge classes; the most used are `FluidStack`, `IItemHandler`, `Capabilities` and the event bus |
| Resources | 5,076 asset files, 4,421 data files |

What that changes in the plan:

- **Mixins are not the hard part.** Nearly all of Create's Mixins map to kinds
  horizon-mixins already has.
- **Catnip comes first.** Create's own utility library (`net.createmod.catnip`) is used
  by half of Create's classes, more than Flywheel or Registrate. Its shim is the first
  dependency to port.
- **Item and fluid transfer belong early in modkit.** `IItemHandler`, `FluidStack` and
  capabilities are among the most used NeoForge classes.

| System | What it is in Java | What the port needs | Hardest part |
| --- | --- | --- | --- |
| Registration | Registrate (a library) builds blocks, items, block entities, models, lang and tags | A Registrate shim on modkit's registries; datagen output carried over as data | Registrate's builder chains are code that *generates data*; run datagen in Java once and carry the output |
| Kinetics | Rotating block entities share a network: speed and stress propagate between them | modkit block entities + ticking; the propagation logic itself is plain game logic and translates well | Exact update order, so networks behave as in Java |
| Machines | Press, mixer, millstone, saw, fan processing, deployer | Custom recipe types, inventories, item/fluid transfer | The deployer acts like a fake player: it needs a modkit hook for using items as a player |
| Belts and logistics | Belts, funnels, chutes, depots, item transport | Item transfer API, entity/item movement on blocks | Item-on-belt rendering and collision |
| Fluids | Pipes, pumps, tanks, fluid networks | A fluid storage and transfer API (vanilla has none) | Fluid rendering in pipes and tanks |
| Contraptions | Moving block structures (pistons, bearings, carts) as entities with their own collision and rendering | Entity hooks, custom collision against moving shapes, rendering a block set as an entity | The largest single system; needs several new Horizon hook points |
| Trains | Track graph, signals, schedules, train physics | Built on contraptions plus a persistent world-level graph | Only after contraptions work |
| Rendering | Flywheel instanced rendering for thousands of moving parts; connected textures; partial models | An instanced rendering hook in Horizon's renderer, exposed through modkit | Needs Horizon renderer cooperation (wgpu) |
| Ponder | Animated tutorial scenes | A Ponder shim: scenes are data plus scripted code | Low priority; can ship without it |
| Mixins | Dozens of Mixins into vanilla | Mixin translation; each one mapped through the port ledger | Mixins that horizon-mixins can't express yet (`@Redirect`, `@Shadow`) |
| Networking and config | Custom packets, config spec | modkit networking and config | Straightforward once modkit has them |

## The experiment (2026-11-10)

How far does `horizon-porter` get on Create's 1.21.1 source today, with no hand work? Run on a
developer's own copy, nothing shared: Create's assets are All Rights Reserved.

**It ports and builds.** `port` reads the source in 16 seconds: 646 blocks, 84 items, 7,759
resource files, 114 block entity types, 113 tick functions and 101 render functions to
translate. The bundle builds on Horizon in six to eight minutes, and in the game all **646
blocks place** with their models, with no faults; 50 of the 60 hand placements tried go
through. What a block *does* is another matter: there are 625 functions to translate, and the
check finds 24 of 195 blocks answering a right click with only twenty of them done.

**Rotation carries.** A creative motor with four shafts on it, a cogwheel on the last, two
more cogwheels meshing above it and a shaft on each:

| Block | `Speed` |
| --- | --- |
| Creative motor, the four shafts, the first cogwheel | 16.0 |
| The cogwheel meshing with it, and its shaft | -16.0 |
| The third cogwheel, and its shaft | 16.0 |
| All of them, a few seconds after the motor is taken away | 0.0 |

They turn on screen, drawn by their block entity while their own model stays out of the world.
Both ticks and both renderers were translated by Claude Haiku from Create's Java: four
functions, about 13 cents with the try that ran too long.

It took these things the porter didn't do before. None is Create's alone:

| What stood in the way | What the porter does now |
| --- | --- |
| A shaft's tick is 20 lines; the work is in `KineticBlockEntity` and `SmartBlockEntity` above it | A block entity takes its fields, saved values and tick from its mod superclasses |
| The tick hands its work to `RotationPropagator`, a helper the prompt left out for smaller classes | Classes whose static functions the code calls are shown first |
| The helper walks the whole network in one go; a tick here sees the six blocks beside it | The tick is told to port a walk as its own step every tick: a change travels a block a tick |
| Whether two blocks connect is asked of the neighbour's class (`IRotate.hasShaftTowards`), in 80 block classes | What each block class answers for the mod's own interfaces is listed in the prompt, and a neighbour's state properties reach the tick |
| The shaft's model is swapped for a class that draws nothing in the world (`BracketedKineticBlockModel`) | Such a block, and one whose `getRenderShape` says so, gets `render_shape: ENTITYBLOCK_ANIMATED` |
| Renderers draw Flywheel partial models named through a helper (`block("shaft_half")`) | Those are found and baked: 245 of Create's, from 20 |
| A cogwheel is registered through a factory of its class (`CogWheelBlock::small`) | The registration is tied to that class |
| A block its entity draws is blank until the renderer is translated | It shows its own model, still, until then |
| No item had an icon: a 1.21.1 mod has no `items/*.json` | A definition is written for each item model of a mod that ships none (743 for Create) |

**Where it stops.**

- **Stress.** Speed carries; stress and capacity don't. `KineticNetwork` keeps them for a whole
  network in memory, by level, and nothing in a port holds state for a level yet.
- **Diagonals.** A large cogwheel meshes with a small one diagonally, which six neighbours
  don't reach.
- **Items drawn by code.** The wrench has no icon: its model is a renderer of Create's own.
- **One function at a time.** Each tick is translated alone, so the shaft's holds a copy of what
  every kinetic block answers (377 lines, and one answer ran past the output limit before a
  second try fit). The next step is to port a mod's own interface once, as a function by block
  that every tick calls.
- **Everything after rotation**: the 600 functions left, then the systems in the table above.
  Belts, fluids, contraptions and trains each need more from Horizon than a hook to paste into.

So: Create is not ported. But "can the tool do anything with Create" has an answer now, and it
is the first step of the order below, done by the tool rather than by hand.

## Suggested order

1. Catnip and Registrate shims, kinetic block entities, shafts, cogs and a hand crank:
   rotation that propagates. That alone proves registries, block entities, ticking,
   saves and simple rendering.
2. Press, mixer and basin, with recipe types and item transfer.
3. Belts and funnels.
4. Fluids.
5. Contraptions, then trains.

Expect each step to surface missing Horizon hook points. Run
`horizon-porter report <Create source>` for the full lists behind the numbers above:
every Mixin with its verdict, the most used Minecraft and NeoForge classes, and the
Minecraft classes Create extends.
