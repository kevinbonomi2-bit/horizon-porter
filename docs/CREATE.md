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

**A machine that works: the millstone** (2026-11-11). A creative motor under a shaft under a
millstone, four wheat dropped on top:

| | |
| --- | --- |
| The millstone's `Speed` | 16.0, from the shaft below |
| Wheat dropped on it | taken into its input slot |
| Every 7.5 seconds (150 ticks at that speed) | one wheat ground |
| After four | 6 wheat flour and 1 wheat seeds in its output slots: the recipe's one flour each, and its two 25% extras rolled |
| A right click with an empty hand | all of it in the player's inventory, the millstone empty |

Its tick, click and renderer are three more functions from Haiku (about five cents). It took
four more things, again none of them Create's alone:

| What stood in the way | What the porter does now |
| --- | --- |
| The millstone keeps an input handler and an output handler; the port gave it the larger one's slots | A block entity's item handlers each get their own slots, one after another, and the tick and the click are told which is which |
| A milling recipe lists several results, each with a chance; a ported `Recipe` had one result | `Recipe::results`, every stack with its chance (it gives Farmer's Delight's cutting board its other cuts too) |
| Items are fed by dropping them on it (`updateEntityAfterFallOn`), which Horizon has no hook for | The block entity's tick is told to take in what is dropped on it |
| The click only tells its block entity what to do (`withBlockEntityDo(.., be -> ..)`), and that class is in the same package, so no import named it | Classes of the same package that the code names are shown to the translator |

Two things found on the way were mistakes of the translation, not of what it was given, and both
are now said plainly to it: turns compose as on a pose stack (the motor's own half shaft tumbled
where it should turn), and a library's helper whose source the mod doesn't carry
(`CachedBuffers.partialFacing`) has to be spelled out.

**The mod's own questions, ported once** (2026-11-11). Whether two blocks connect is asked
through Create's own interfaces (`IRotate.hasShaftTowards`, `getRotationAxis`,
`ICogWheel.isSmallCog`), answered by some thirty block classes. The first shaft tick carried its
own copy of every answer. Now each such method becomes one function of the port, over the block
and its state properties, with every class's answer quoted in its comment for the translation:

| | Before | After |
| --- | --- | --- |
| The shaft and cogwheel tick | 377 lines, one answer cut off at the output limit | 158 lines, asking `i_rotate_has_shaft_towards` of both blocks |
| What decides a connection | each tick's own table, differing from tick to tick | seven shared functions, about four cents to port |
| A millstone or a crank beside something new | connects only to what its own table knew | connects to anything the shared functions know |

The hand crank turns with it: its click sets its counter, its tick takes its speed from that
every tick, and a shaft and a cogwheel on it read -32 while it is held and 0 after. Both were
ported apart and didn't agree before; what changed is that the click's translation now sees the
block entity's class, and the tick was ported again.

**Six more kinetic blocks, and what a block entity passes on.** With the shared functions, an
encased shaft, an encased cogwheel, a gearbox, a clutch, a gearshift and a water wheel were
translated (about two cents each). At first they connected and passed speed on unchanged:

| Block | First | In Java, and in the port now |
| --- | --- | --- |
| Encased shaft | passes 16 on | the same |
| Gearbox | 16 to its three other faces | -16, -16 and 16: each face turns its own way |
| Clutch | follows redstone, still passes 16 | passes nothing while powered |
| Gearshift | follows redstone, still passes 16 | passes -16 while powered |

What a block passes on is asked of the block entity giving the speed
(`SplitShaftBlockEntity.getRotationSpeedModifier(face)`, a gearbox's faces in
`RotationPropagator.getAxisModifier`), by the one taking it, whose tick is ported alone. So the
same step was taken one level up. A method the block entities' classes answer differently, and a
static helper that takes block entities, each become one function of the port too, over a view of
the block entity (its block, its state, where it is, its saved values):

- `split_shaft_block_entity_get_rotation_speed_modifier(be, face)`, with the clutch's and the
  gearshift's answers quoted for the translation;
- `rotation_propagator_get_rotation_speed_modifier(from, to)`, `get_conveyed_speed`,
  `get_axis_modifier` and `is_connected`, ported from `RotationPropagator` and calling the
  questions above and each other, as the Java does.

Every kinetic tick now takes its speed through `rotation_propagator_get_conveyed_speed(neighbour,
me)`: ten ticks, 44 to 146 lines each where the first shaft tick alone was 377, and all of them
agree, because none carries its own idea of what connects or what is passed on.

**A press over a depot** (2026-11-11). A creative motor beside a mechanical press, a depot two
blocks under it:

| | |
| --- | --- |
| The press's `Speed` | -16.0, from the motor beside it |
| Sixteen iron ingots, right-clicked onto the depot's top | all sixteen on the depot |
| Every 6 seconds (a 240-step cycle, two steps a tick at that speed) | the head comes down and one ingot becomes an iron sheet, as with Create's bulk pressing off |
| The motor set to 64 | a sheet every 1.5 seconds: eight steps a tick, as `getRunningTickSpeed` gives |
| The depot after three | 13 ingots held, 3 sheets in its output buffer |
| A right click with an empty hand | 13 ingots and 3 sheets in the player's inventory, the depot empty |
| A press placed by hand beside a shaft | turned to take it (`facing=west`), not left as it came |

The press's tick is one more function from Haiku (about four cents a try). What it took was not
the press: five things the porter didn't do, each of which had been costing every mod something.

| What stood in the way | What the porter does now |
| --- | --- |
| The press gives the depot code to call (`whenItemEnters`, `whileItemHeld`) and the depot's tick calls it. Each tick is ported alone, so nothing ran | For each block entity, the classes of *other* block entities that take something it holds and run the code it gave are found. Its tick is told who they are, how they find it (`pos.above(2)`), and which of their slots hold what; their Java is read, and the tick does that work on them itself. In Create: the press, the spout and the deployer |
| `state.getValue(HORIZONTAL_FACING)` became `"horizontal_facing"`; the property is `facing`. Eleven functions of the port read or set a property no block has: the press never connected to a shaft, and five kinds of block were placed without their facing | The prompt names each property whose name isn't its constant in lower case, the mod's own included (`AXIS_ALONG_FIRST_COORDINATE` is `axis_along_first`); an answer that uses the constant's name is sent back; `translate --refresh` finds ports that already have one |
| The pressing recipe asks for `c:ingots/iron`, a tag NeoForge provides. The porter added the loader's common tags a mod's data names, but only as `"#c:.."`; a 1.21.1 recipe says `{"tag": "c:.."}` | Both forms are read. Create's port carries 149 common tags where it had 67, which its crafting recipes needed as much as the press |
| `AllConfigs.server().recipes.bulkPressing.get()`: the default is in a class nothing imports, and the translation guessed it | Config values the Java reads are listed with their declaration and its default (`false`), and the tick is told a port always runs on the default |
| In Java a block entity ticks on the client too, so the press's head moves every tick. A port ticks it on the server alone, and the head jumped from up to down | The tick's helper draws the block entity from its state before and after the tick; when the picture differs, clients are sent the state again. An idle machine sends nothing |

Two smaller ones: a kept port that is to read more Java now (the press's, once the depot's class was
named) is marked for `translate --refresh`; and `context.getClickedPos()`, which in a placement is
where the block *goes*, is spelled out, after the hand crank counted its neighbours from the block
that was clicked. The first of these caught the creative motor too: its tick was translated before
its speed setting was part of its state and always made 16. Ported again, it makes what `ScrollValue`
says. Nothing sets that in the game yet (the box on the motor's back is a screen of Create's own), so
a faster motor is `/data merge block <pos> {ScrollValue:64}` for now.

What it still doesn't do: a stack dropped under the press with no depot is pressed whole (the
translation wrote the comment for one item a cycle and the code for all of them); the depot's
`locked` flag has nowhere to live, so a funnel could take an item mid-press; items on a belt under
the press aren't reached, belts not being ported; and the basin under a press is the next step.

**A mixer over a basin** (2026-11-12). A basin, a mechanical mixer two blocks over it, a cogwheel
beside the mixer on a motor set to 64:

| | |
| --- | --- |
| The mixer's `Speed` | -64.0: it meshes with the cogwheel as the small cog it is |
| Andesite and iron nuggets dropped into the basin | taken into its input slots; the mixer is told and starts |
| Every 2.3 seconds (46 ticks: `log2(512 / speed) * 15 + 1`) | one andesite alloy in the basin's output slots |
| Below 30 RPM | the mixer's tick holds off, by its code (not tried in the game): `mediumSpeed`, read from Create's config class |
| A right click with an empty hand | everything in the player's inventory, the basin empty |

Six functions from Haiku (about 20 cents), and again what it took was in the porter:

| What stood in the way | What the porter does now |
| --- | --- |
| The basin's inventories are a class of Create's own (`BasinInventory extends SmartInventory`), and its tanks sit in a behaviour made with a count and a capacity. The scan gave the basin 2 slots and an empty tank | A mod's own item handler classes and tank holders are found, and making one makes storage: 9 input slots, 9 output slots, four 1000 mB tanks. A handler that only stands in where a capability is missing (`if (x == null) x = new ItemStackHandler()`) is none |
| The basin tells the mixer its contents changed (`getOperator().basinChecker.scheduleUpdate()`); its tick had no way to know the key the mixer keeps that flag under | A tick is shown the saved keys of the other block entities its Java names: the basin sets `NeedsUpdate` in the mixer two blocks up |
| "Is it a small cog" was ported as "not a large cog", for every block: two shafts side by side would have driven each other | A question to a block now says which blocks are of its interface at all, and that Java never asks the others |
| A function ported again kept its old mistakes: its last port was still in the prompt, as part of the module shown | It isn't shown. The mixer's speed limit went from a guessed 32 to the config's 30 on the next try |

What it doesn't do yet: fluids (the basin has its tanks, but a tick can't yet reach the tanks of
another block entity, so recipes with a fluid don't run), heated recipes (no blaze burner),
the basin passing its output on to a depot or belt beside it, and the press on a basin. That last
one is the limit of one function at a time: asked for depot, world and basin modes together, the
press's tick ran past 48,000 tokens of answer and was left as it was. `BasinRecipe`'s matching
and applying want to be ported once and called by both, as the questions are.

**Porting it again from nothing** (2026-11-12). Would someone else, with the same tool, get the
same Create? To find out, Create was ported into an empty folder and the same 59 functions
translated in one run (about $1.60), then checked in the game by a script that builds every rig
above and reads what the block entities hold (29 checks).

| | Checks passing |
| --- | --- |
| The port grown over the last days | 27 of 29: no diagonal cogwheels yet |
| The fresh port, as first translated | 24 of 29 |
| After `translate --refresh` found and redid five functions by itself | 28 of 29 |
| With the press's tick taken from the earlier port | 29 of 29 |

What the fresh port showed, each of it a thing a first-time user would have hit:

| What went wrong | What the porter does now |
| --- | --- |
| It didn't compile: a question to a block with nothing more to ask got a stub that wasn't Rust | Fixed, and a fixture's fresh port is compiled in a test |
| Axles stood still: renderers called the shared angle helper, which hadn't been translated and answered 0; once translated it had no clock | `translate` ports the shared functions a ported one calls; `render_time()` is the frame's clock for a function that isn't handed the frame |
| The millstone never ground: the recipe's time was read as `processingTime`, and the files say `processing_time` | The value names the mod's recipe files carry are recorded and shown; an answer that reads another is sent back |
| The crank had no handle: `create:block/hand_crank_handle` was drawn, a model made up from the constant `HAND_CRANK_HANDLE` | The models the Java names are given by the id each is baked under; an answer that draws another is sent back |
| The large cogwheel lay flat and wobbled: `partialFacingVertical` is a library's, and its turn was a guess | Spelled out, like `partialFacing` |
| The basin took nothing in: the faithful translation asks which block the item is in, and the basin is solid here where Java lets items drop into it | The tick is told an item resting on top has landed |
| The wrench, the nuggets, raw zinc and dough showed as nothing: `.tag(Items.TOOLS_WRENCH)` was read as the vanilla item to copy | A tag isn't a stand-in, and a stand-in is checked against vanilla's items |
| The clipboard couldn't be opened from the hand: a block's item with a class of its own got no function for its `use` | It does (31 of Create's blocks have one) |

Every one but the first is the translation guessing something that wasn't in front of it: a name,
a default, a turn. So is every slip found before it (property names, config defaults, which blocks
are cogwheels). They are closed the same way each time: the porter states the fact, and where the
answer can be checked against what exists (a property, a model, a recipe value), a wrong one doesn't
get through. That is what makes two people's ports agree; the translation itself never will be
the same twice.

One function doesn't come out again: the press's tick. With the depot, the dropped items, the basin
and the kinetic walk in one function the answer runs past 48,000 tokens, and asked to be shorter it
drops the depot. The one in the port now is from the earlier run. Its cure is the one already named:
the kinetic step, the same in sixty block entities, ported once.

**The rest of it.** 715 functions are left to translate, with what stands behind each kind:

| Kind | Left | Works once translated? |
| --- | --- | --- |
| Block placement and what follows it | 137 + 125 | mostly: the large water wheel builds its frame this way |
| Block entity ticks | 112 | those that only need what a tick has; belts, fluids, contraptions need more |
| Renderers | 101 | yes, with the models the Java names |
| Block clicks | 81 | mostly; a click that opens a screen of Create's own (value boxes, the clipboard) needs screens |
| Item use | 25 + the new block items' | the wrench turns blocks through an interface each block answers, with effects: not a plain question yet |
| Overlays | 0 of Create's 9 layers have a function | the goggles' panel is one: registered layers aren't read yet |

**Where it stops.**

- **Stress.** Speed carries; stress and capacity don't. `KineticNetwork` keeps them for a whole
  network in memory, by level, and nothing in a port holds state for a level yet.
- **Diagonals** work now: a tick whose Java walks the cube around it reaches the block
  entities there, and a large cogwheel turns a small one at half the speed, the other way.
- **Items drawn by code.** The wrench has no icon: its model is a renderer of Create's own.
- **One function at a time.** Each kinetic block entity's tick still carries its own copy of
  the step that takes speed from its neighbours: it is `KineticBlockEntity`'s work, the same for
  all hundred of them, and translated again for each (about two cents and 50 to 170 lines a
  time). Porting a base class's tick once, for every block entity that inherits it, would end
  that.
- **The whole of it** is 662 functions to translate: about 45 million tokens in, some $5 with
  Claude Haiku for one try each, and more with the tries that need fixing.
- **Everything after this**: the 600 functions left, then the systems in the table above.
  Belts, fluids, contraptions and trains each need more from Horizon than a hook to paste into.

So: Create is not ported. But "can the tool do anything with Create" has an answer now: the
first step of the order below and the first machine of the second, done by the tool rather than
by hand.

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
