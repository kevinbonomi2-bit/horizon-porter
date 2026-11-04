# Case study: what porting Create takes

[Create](https://github.com/Creators-of-Create/Create) is the benchmark for
"full mods". This page breaks it into the systems a port must cover, and what each
one needs from horizon-porter, modkit and Horizon itself.

**Licence.** Create's `LICENSE.md` puts its **code under MIT** but its **assets
(textures, models, sounds) under All Rights Reserved**. A port may translate the code,
but it must not redistribute Create's assets: they have to come from the player's own
copy of the mod, the way Horizon's launcher handles modpacks.

**Version.** Create's main branch targets Minecraft 1.21.1; Horizon is 26.3. The Java
mod has to reach 26.3 first.

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
