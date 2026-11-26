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
| After `translate --refresh` found and redid five functions by itself, and with the press's tick taken from the earlier port | 29 of 29 |

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
- **The whole of it** is 700 functions to translate: see "What it costs" below.
- **Everything after this**: the 600 functions left, then the systems in the table above.
  Belts, fluids, contraptions and trains each need more from Horizon than a hook to paste into.

So: Create is not ported. But "can the tool do anything with Create" has an answer now: the
first step of the order below and the first machine of the second, done by the tool rather than
by hand.

### What it costs (2026-11-12)

The figure this page gave before, "some $5" for the whole of Create, was wrong by nearly ten
times, and so was what `translate` printed. Two things were off:

- **Tokens.** The estimate took four characters to a token. Create's prompts (Java and
  generated Rust) are 2.3 characters to a token on Claude Haiku 5.5, counted by the API.
- **Price.** Haiku 5.5 costs $0.10 a million tokens in and $0.50 out only while the prompt is
  100,000 tokens or fewer; a longer one costs five times as much, in and out. Create's
  prompts averaged 225,000 tokens.

So a function cost about 12 cents, not one or two, and the 125 or so translated for this page
cost about $15 that the tool recorded, more with the answers that were cut off and asked
again (those were not counted at all). The whole of Create would have been about $85.

What changed in `translate`:

- A prompt is priced from what the API says it used, at the card its own length picks, with
  cache writes and reads at their own rates. Requests whose answer was cut off are counted.
- A class is sent without its `import` lines (a fifth of Create's Java). A class the code uses
  is cut to the methods that code reaches, the rest by signature.
- Of a large module, a function called from here is shown by the first paragraph of its
  comment and its signature; a state's `load` and `save` by their signatures.
- The part of the module every function with the same parameters is shown (the types they
  reach, the generated functions and tables) is sent first, the same text each time, and read
  from the API's cache at a tenth of the price. Measured: the nozzle's tick read 11,482 of its
  36,298 tokens from the cache; the press's, 22,539 of 84,088 on each request.
- No prompt is written past 90,000 tokens: the classes it uses are cut back first.

The press's tick, the largest prompt, went from 225,000 tokens or more to 85,299 (counted).
The estimate for the 678 functions then left: 34 million tokens in and **$3.66**, one try
each. Three ticks translated on those prompts cost $0.004, $0.004 and $0.041.

The $0.041 is the press, and it is the other finding. Haiku thinks before it answers, and
its thinking counts against the room for the answer. On the press it used all 16,000 tokens
thinking and wrote nothing, twice in earlier runs with room for 48,000 too. Asked again with
less thinking and room for 48,000 it wrote a 319-line tick that compiles. Asked instead with
thinking off, it wrote a tick that did nothing but say the Java wasn't ported, and that
compiled and passed the tests too: it had replaced a working port. So now a cut-off answer
that was all thinking is asked again with less of it, thinking is turned off only when that
fails as well, and an answer with no statement of its own under a note that nothing is ported
is sent back; twice, and the function is left as it was.

### The shared kinetic step, checked (2026-11-12)

The build with the ticks ported once (17 of them, with the 12 block entity ticks ported again
around them) passed 20 of the 29 checks, down from 29. What broke, and what the tool does
about each:

- **A large cogwheel no longer turned a small one.** `SimpleKineticBlockEntity` overrides
  `addPropagationLocations`, which places a block meshes with. The shared tick is ported from
  `KineticBlockEntity` alone and knew only its answer. A method that takes or gives back a list
  of places can be a question to a block entity now, ported once with every class's answer.
- **The crank did nothing when clicked.** Its tick left `updateGeneratedRotation` to the class
  below, which did not do it, because `getGeneratedSpeed()` is each class's own. A method the
  shared classes call on the block entity itself and a class below overrides is a question
  too now, and the shared tick's comment names the function that answers it. Create has 36
  such questions.
- **The click set a key no tick reads.** A click cannot run `updateGeneratedRotation()`; Java's
  own tick has a switch for it (`if (reActivateSource) { updateGeneratedRotation(); .. }`). The
  prompt for a function without a tick's reach now names that switch and its key, read from
  the Java.
- **The motors stayed at 16.** That one was the check: it set `ScrollValue` with `data merge`,
  which changes nothing in Java either. It sets `reActivateSource` with it now, which is what
  scrolling the value does.

More came out on the way, each fixed in the tool: a tick's view of its own block entity carried
no block entity id (a generator bug: every question that tells classes apart by it answered for
none); answers compared a block's id with a block entity's (`create:motor` is the block entity
of `create:creative_motor`), which is sent back now; a position kept as text was written
`x,y,z` by one function and read `x y z` by the rest, so there is one `pos_text` and one
`text_pos`; a tick is told by name which methods its own classes override, and one that never
calls its shared tick, or returns before the work others call it for, is sent back.

The press and the mixer took four more, each a fact the tool now states or checks:

- **A recipe a block entity found** (`Recipe<?> currentRecipe`) had no place in its state, so the
  tick that found it could not hand it to the tick that applied it. It is kept by its id now.
  The fields of the classes above that still have no place are listed for each block entity,
  with what to do about one the Java only looked up.
- **A kept part made with the block entity's own method** (`new DeferralBehaviour(this,
  this::updateBasin)`): the tick is told how it was made, the method is shown in full (it was
  cut to its signature, since nothing called it by name), and a tick that never runs it is sent
  back.
- **Another block entity's item slots** (the basin's inputs are slots 0 to 8, its outputs 9 to
  17) are listed with its saved keys; the mixer had been told only the keys.
- **A class a shown class hands its work to** (`BasinRecipe.apply` from a base class's
  `applyBasinRecipe`) is shown too.

Where it stands: **29 of 29** on the build with every tick above ported once
(native-66f3981-268caca8ada7), from the tool's own output with nothing edited by hand. Getting
there took about a dollar of Claude Haiku in all, the dead ends included. Twenty-odd functions
of the port are still marked to be ported again with what the tool says now; none of the
checks depends on them.

### Every function translated (2026-11-13)

Played by hand, the port had machines that placed one way only and had none of their moving
parts: a fan without its blades, a saw without its blade, encased cogwheels without the cog,
gearboxes and clutches without their shafts, a track that couldn't be broken. Behind it, 680 of
the port's functions were still stubs: 93 renderers, 80 placements, 55 shapes, 96 ticks, 91
clicks and the rest. They had been left because translating them one after another took a day
and, at the old prices, $85.

- **`translate --jobs N`** (4 by default) asks the model for several functions at once. Each
  answer is still put in the file and tested on its own, so the file passes its tests between
  any two of them. Six at a time, Create's 710 functions took about two hours.
- Translated: 710 of them, for $7.30 of Claude Haiku. 27 are left: the model's answers to them
  did nothing but say the Java isn't reachable (train screens and packets, the goggles' overlay,
  copycats, a few renderers that only draw value boxes).
- **Machines face the player** as in Java: a fan, saw, deployer, drill, piston or bearing placed
  from the west faces west, from the north faces north (placed by hand through `horizon-ctl`).
  A gearbox goes down upright, as in Java, where the wrench turns it: the wrench's use is ported
  too, every block's own `onWrenched` and rotation with it.
- **Moving parts are drawn**: shafts inside gearboxes, clutches and gearshifts, the cogs of the
  encased cogwheels, fan blades, saws, the deployer's hand, the press and mixer heads, the
  water wheels, the flywheel, gauges, the arm, bells and the rest of the renderers.
- **The item on a depot sits on it.** Its renderer turned the item about the block's centre
  after moving to the depot's top, so the turn threw it a block up. A turn Java makes with
  `mulPose` after a `translate` is about the moved origin, and an answer that turns about the
  centre there is sent back.
- **A track can be clicked and broken.** Its models are NeoForge OBJ models, which have no
  boxes for the porter to take a shape from, so the track had none. An OBJ model's shape is now
  the bounds of its vertices, cut to the block.
- **Answers that compile and do nothing are sent back** in more cases: a tick that names its
  shared tick only in a `// Missing:` note, a renderer that turns about a pivot after moving.
  The press's tick had been replaced by one that did the first; the checks caught it, and it
  was translated again.

All 29 checks pass on this build (native-66f3981-a7ceda70e323). `ai-test/create_gallery.py`
photographs each of the 135 blocks with a block entity, powered where it turns, and puts the
pictures in sheets of twelve; `ai-test/create_place.py` places blocks by hand from several sides.

Still not there, by what it needs:

- ~~The motor's speed, and every value Create sets in a box on a block~~: done since, below.
- **Contraptions**: pistons, bearings, pulleys, gantries and the rest move blocks as one
  structure, an entity Horizon has nothing like. They place, turn and draw their parts, and move
  nothing.
- **Trains**: tracks place as straight pieces; curves, stations and trains need the same.
- Smaller: the blaze in the blaze burner, the peculiar bell's bell, copycat blocks (drawn from
  the block they copy), the ascending track's collision (a full block here, none in Java).

### Values set in a box on a block (2026-11-13)

A creative motor's speed is set in Java by looking at the box on its side, holding right click
until a board opens, dragging along it and letting go. Nineteen of Create's block entities do
the same with a value of their own: the speed controller's target, a valve handle's angle, the
bearings', pistons', pulleys' and cart assembler's movement modes, the ejector's stack size,
the brass diodes' delays, the brass tunnel's and the arm's selection modes, the roller's mode,
the chassis' range, the steam engine's direction. All of them are Create's
`ScrollValueBehaviour` (or a class extending it), so the porter now knows that framework
(`crates/porter-translate/src/value_settings.rs`), and a port of Create's add-ons gets it too:

- **Generated, as Create does it for every one of them:** the use key held on a box for five
  ticks opens the board (`ValueSettingsInputHandler`, `ValueSettingsClient`); the board is
  drawn from Create's own texture, with its rows, milestones and the cursor's text
  (`ValueSettingsScreen`), shift for milestones only, the wheel a step at a time; letting go
  sends the choice to the server (`ValueSettingsPacket`, channel `create:value_settings`),
  which sets the value clamped to its range, runs its callback, plays Create's two sounds and
  sends the block to the clients. Looking at a block shows each box with its value, and the
  one under the crosshair gets an outline and Create's tip ("Click and hold to edit").
- **Translated, once each:** for each value, what its block entity's `addBehaviours` sets up
  (label, range, formatter, when it shows), where its box is (its `ValueBoxTransform`, with
  Create's rotation helpers given as functions so the turns match Catnip's), and its callback
  (the motor's `updateGeneratedRotation()` becomes the switch its tick reads); for each
  behaviour class, its board, where its value sits on the board and what a choice sets (the
  motor's rows pick the way it turns).

The box was first drawn over the world from the HUD, which jittered as the player moved. It is
now drawn with its block, as `ValueBox.render` does: the value in a 3×5 pixel font filling the
box, an outline round the one looked at, a little in front of the box's anchor (a motor's sits at
12.5/16, inside its side plate), depth-tested so it stays put. Horizon has no text drawing for
block entities, hence the pixel font; the tip stays on the HUD.

Checked in game (`ai-test/create_dial.py`, builds native-66f3981-db82e152d283 and
-a8bfa6454f9d): looking at a creative motor's side shows its speed and Create's tip; holding
right click opens the board on the motor's speed (16 on the ⟲ row); dragging 40 pixels right
and letting go set it to 52, and the shaft beside it turned at 52; dragging left past the start
set 1, as Java does. All 29 checks still pass. The motor's and speed controller's nine
functions were translated by Gemini (`gemini-3.6-flash`, free; `gemini-3.8-flash` answered
only "high demand" that night).

Then every other value (`ai-test/create_values.py` places each block so the face it looks at
carries a box, photographs the box and its tip, holds right click and photographs the board):
all 14 blocks open their board on build native-66f3981-3615c9927eb3, with Java's labels, ranges,
rows and option names (the bearings' "Always Place when Stopped", the arm's "Round Robin", the
clockwork bearing's "Hour hand first", the diodes' ticks/seconds/minutes rows, the valve
handle's ⟳/⟲ rows to 180°, the chassis' "Distance" with a wrench in hand). The elevator pulley
has none, as in Java. Getting there showed what a value's port needs to be told, now all in the
porter:

- **Where a box is** is often not in the class that makes the value: a method makes its
  `ValueBoxTransform` (`getMovementModeSlot()`, a default method of `IBearingBlockEntity`, or
  `PulleyBlockEntity`'s override, where the "top only" rule is), and that class extends another
  (`CenteredSideValueBoxTransform`, where the position is). Those classes are named in the
  prompt now, with `VecHelper.rotate` given as a function beside `rotateCentered`.
- **Types nested in a class** aren't in the scan: an option enum reached by inheritance
  (`RotationMode`, declared in `IControlContraption`) or a behaviour class declared inside its
  block entity (`ValveHandleBlockEntity.ValveHandleScrollValueBehaviour`). The class they are in
  is named instead, and a nested behaviour class's own methods are ported from its body.
- **A class that doesn't call `super.addBehaviours`** never has its parent's values (the
  elevator pulley), so they are left out.
- **Lang keys**: `translate` now sends back an answer that names a key of the mod's that its
  lang files don't have (`create.gui.contraptions.rotation_mode...`, made up), or Create's key
  without its `create.` (`generic.unit.ticks`), with the keys it may have meant.
- Option values span all their options and their box shows the chosen one, and their board is
  drawn in icon mode (the option's name under it), whatever a port said; a start set with
  `x.setValue(45)` is read like `x.value = 45`.

Translated for these: 51 functions by `gemini-3.5-flash-lite`, 17 box functions again by
`gemini-3.5-flash` (Lite placed boxes wrongly), the bearing's options by Claude Haiku ($0.01,
Lite twice took the piston's enum). Google's free tier gave 20 requests a day per flash model.

Still short of Java: an option's icon isn't drawn (Create's icon sheet), a value Java starts
from a method or a config (`defaultValue()`, `max / 2`) starts at 0 here (the diodes, the
chassis), and the valve handle's zero column reads 0° where Java shows 1°.

### What playing it showed (2026-11-13)

A round of play turned up machines that ran but not as in Java. Each was a gap in the porter,
fixed for every mod rather than for Create:

- **A fan's blades spun off their axis, the arm looked wrong:** Java draws each part from its
  own `SuperByteBuffer` with its own turns; the ports turned one pose for all of them.
  `translate` now sends back a renderer that does that, and says to push and pop per part.
- **The cuckoo clock's hands ran unpowered and nothing happened at noon:** Catnip's
  `LerpedFloat` is a type of the port now (chase, `tickChaser`, `getValue(partial)`), started as
  the Java starts it, and a tick can read the day's time (`machine.day_time`). Tick stubs say
  that what Java does on the client side of a tick to its state is done in the server tick.
- **The valve handle drew a crank inside it, a gauge drew no shaft:** a renderer's stub names
  the renderer classes above it and the block entity whose overrides it calls.
- **Tracks were placed diagonally:** a Registrate entry made by a method reference
  (`TrackMaterial.ANDESITE::createBlock`) now takes its class from the field's declared type
  (`BlockEntry<TrackBlock>`), so the track's own placement runs. They lie flat.
- **Crushing wheels spun but crushed nothing.** Five gaps, each now closed:
  - the controller between the wheels is a block the wheel's code places two blocks out
    (`pos.relative(side, 2)`): `machine.set_block_at` puts a block in the world after a tick;
  - its `ProcessingInventory` fixes its own size (`super(32)`) and is made with a callback,
    so the scan counted no slots: an item handler class of the mod's own that fixes its size
    now counts wherever it is made;
  - the wheel sets the controller's speed through the block registered for it
    (`AllBlocks.CRUSHING_WHEEL_CONTROLLER.get().updateSpeed(..)`), whose code the translator
    never saw: a call through a registered entry, or through a cast to a mod class, now shows
    that class's code, and names its block entity's saved keys (`crushingspeed` → `Speed`);
  - the controller's `facing` (which way it drops what it crushed) was missing, because its
    blockstate file draws by `valid` alone: Java's direction and axis properties
    (`FACING` from `DirectionalBlock`) now join a block's states, starting where Java's do
    (`down`). Five of Create's blocks gain one, each as its Java has it;
  - what it drops it took straight back in and crushed again, gravel to sand to nothing: Java
    tags its output (`BypassCrushingWheel`). `machine.eject` drops an item marked as the block
    entity's own, which its `dropped_in` never lists again.

  In game (build native-66f3981-09aa360864cf): the controller carries Java's speed (0.32 for
  16 RPM), a cobblestone drops out below as gravel, four gravel as four sand and a flint, at
  Java's rate (the speed over log2 of the stack). All 29 checks still pass.
- **Placing a crushing wheel looks odd:** the axis it ends with is Java's (where the player
  looks, else a neighbouring shaft's), but Horizon asks a mod's placement only on the server,
  so the client first shows the template's guess (the clicked face's axis) for a tick. That
  needs Horizon to ask the mod when the client predicts a placement.

Translated for this, with Claude Haiku: the wheel's and the controller's ticks ($0.08), then
the controller's again once it could eject ($0.06).

Still short then: pipes and brackets, and network stress (both done since, below).

### Checked against real Create, every block (2026-11-13)

Rather than fix what a player happens to notice, every block is now checked against real
Create running beside the port. Real Create (6.0.11 on NeoForge 21.1.219, from its own Gradle
setup with a JDK 21, all local in `C:/Users/kevin/create-ref`) runs a small harness of ours
(`horizonref/HorizonRef.java` in a clone of the source): it makes a flat creative world,
records every block's default state, shapes and block entity, places each block item in nine
ways a player would (the floor from four sides, looking down, two walls, the ceiling, beside a
shaft) and records the state it takes, stands every block in a grid with creative motors
beside the kinetic ones and records its state, saved values and speed, photographs each
(twice for the turning ones), right-clicks the 36 that open a menu and photographs the
screen, then quits. The port runs the same through `horizon-ctl` (`ai-test/create_audit_*.py`,
local): the same grid, motors, camera and placements. Create's own 64 game tests (`runGameTestServer`)
all pass in real Create; 15 of them read as specs (`ai-test/create_gametests.py`) and are
run in the port by placing their structures (`ai-test/create_gametests_run.py`).

What the comparison found, and what the porter does now (all general):

- **Default states** were the template's where Java's `StateDefinition.any()` decides: a
  property no constructor sets starts at its first value (`true`, `x`, `floor`, `north` for
  `FACING`, an enum's first constant), defaults merge down the class line, and a property
  declared as another class's constant is that one. 507 of 513 default properties now match
  Java (371 before); blocks turned by the motors at the wrong speed went from 26 to 1.
- **Enum and direction properties** Java declares but the blockstate doesn't draw by (the
  blaze burner's `blaze`, the controller's `facing`) are declared.
- **Custom baked models** (`PipeAttachmentModel`, `BracketedKineticBlockModel`, belt, tank,
  track, factory panel, girder, copycats) are drawn by functions translated from the class,
  with the block entity: pipes draw their rims, connections and brackets. Part models named in
  loops (`"fluid_pipe/" + type + "/" + d`, `color + "_valve_handle"`) are found as patterns.
- **105 dyed blocks** (valve handles, sails, postboxes, table cloths, nixie tubes,
  toolboxes, seats) had no class and no block entity: registrations made per dye colour are
  expanded, and list constants (`DYED_VALVE_HANDLES.toArray()`) hold them. Dyed valve
  handles draw.
- **Kinetic networks** add up: Create's stress values come from the registrations
  (`CStress.setImpact`), each member reports its part, and stressometers read the network
  (a fan at 16 RPM: stress 32, as Java).
- **Placements**: a block whose class extends one of the mod's with its own placement calls it
  for `super` (encased cogwheels); shapes are the whole block where Java defines none.
- **Item transport**: a tick reaches the containers beside it (`machine.container`,
  `take_from`, `give_to`), and kept items keep their counts (chutes).

Translated for this pass with Claude Haiku: about $0.75 in all.

Still short, by what the audit lists:

- **Menus**: none of Create's 12 container menus is ported (toolbox, postbox, frogport,
  schematicannon, schematic table, redstone requester, filters, blueprint, linked
  controller, schedule, stock keeper). Their layouts and inventories are Create's own
  (`MenuBase`, compartments, ghost slots) and need a menu port of their own.
- **Template properties** Java's block doesn't have (latches' `delay`/`locked` from a
  repeater, a schematic table's `has_book`): Horizon's content format adds a block's
  properties to its template's and can't take any away, so a better template is needed.
- **Connected textures** (casings, pillars' `north`..`west`, tanks, girders), **track tilt**
  and **copycats** change the block's own quads, which needs Horizon to let a mod change a
  block's mesh.
- **Blocks Java removes when alone** (a belt, a cart assembler off a rail) and **belt
  funnels** that turn into funnels off a belt: their `updateShape`/`canSurvive`.
- **Game tests**: 1 of 15 pass in the port (wheat milling; see "Item transport" below).
  The smart observer tests still fail (the observer's item and fluid detection), and fan
  processing, compacting, deploying (the deployer test passed by accident before: its
  piston fired at placement) and sequenced assembly use recipe types no machine of the
  port runs yet.
- **Placing** still differs for some (bells' attachment, table cloths, ladders against
  shafts), and Horizon shows its template's guess for a tick before the mod's placement.
- Trains, contraptions and the 22 functions Haiku declines (trains' packets, the potato
  cannon, goggles) are untouched.

### Item transport (2026-11-13, night)

Checked in game on build `native-66f3981-b8a34721ba9c` ("porter trials"), 29 of 29 checks:

- **Hoppers and machines**: a hopper takes flour out of a millstone and the wheat milling
  game test passes (6 s). The port's container hook asked its chest-like table first, and
  60 Create blocks there (millstone, basin, depot, chutes) had no menu, so Horizon saw a
  container no side could reach; the item handler rules come first now.
- **Belts**: built with the belt connector as a player does, a belt walks its chain on its
  first tick (the ported `BeltBlock.initBelt`: controller, length and index on every
  segment), turns with the motor (16 RPM on all six segments), takes an item dropped on
  its controller segment, carries it at Java's speed and throws it off the end it runs to.
  `ai-test/create_belt.py` does this through horizon-ctl.
- **Smart chutes** are full blocks again (they were templated as a pressure plate, too
  thin to hold their lever).

Porter gains behind these: lists of the mod's own objects kept in a block entity's state
(`transported_item_stack_list`), `machine.entity_at` for any loaded block entity, static
helpers that work on the level as functions of their own (`belt_block_init_belt(pos,
machine)`), answers quoting the methods they call (`BeltBlockEntity::hasPulley`), and
prompts quoting the short methods they call on classes too long to show.

Still short on belts: only the controller segment takes dropped items (Java takes them
on every segment, through the controller), the ends don't insert into funnels, tunnels,
depots or containers, and processing on the belt (fans, presses, deployers) and
entities riding it are left out. A belt placed from a game test structure keeps Java's
`Controller` position, which the port doesn't read. Haiku for this pass: about $0.74
(the night's total about $1.69 of the $5 approved).

### Belt ends and funnels (2026-11-13, later)

Checked in game on build `native-66f3981-c44e6aaa728a` ("porter trials"), 29 of 29 checks;
`ai-test/create_belt_ends.py` builds it as a player would, 3 of 4 pass:

- A **pushing belt funnel** takes a chest's iron ingots out one by one onto the belt below.
- The **belt's end hands an item to a depot**; the depot holds one, and the rest queue a
  block apart behind it, as in Java.
- A **pulling belt funnel** over a segment takes what passes under it into the chest
  behind it (7 of 7, with nothing lost or doubled).
- Still failing: an item dropped on a segment other than the controller's isn't taken in
  (the belt tick still notes it can't hand it to the controller).

What made it work, in the porter: a behaviour method asked of another block entity
(`DirectBeltInputBehaviour.handleInsertion`, `InvManipulationBehaviour.insert`,
`FilteringBehaviour.test`, ...) is a function of its own, ported once for every block
entity that sets the behaviour up (nine take belt input: belts, depots, funnels, chutes,
basins, millstones, saws, crushers, drains); Create's belt helpers over its kept
`BeltInventory` (`checkForFunnels`, the tunnel and crusher checks) are functions of their
own; ticks reach containers anywhere (`give_at`, `take_at`); fields start where Java's
constructors start them (a filter's 64 and "up to"); and prompts quote the short methods
a function calls on classes too big to show. Haiku for this round: about $1.00 (the
night's total about $2.69 of the $5 approved).

Seen and not yet checked against Java: two items put on the same spot of a belt are
spread out backwards, sometimes past the belt's last segment, before they move on. A
belt funnel's `facing` is stored half a turn from Java's (as its model is drawn), so a
test or command that sets one by hand gives the opposite of Java's facing.

One thing to watch: re-translating the item ticks also re-translated the shared kinetic
step they call, and the new one squared speeds (`getConveyedSpeed` already includes the
speed). The 29 checks caught it; the working one was put back, and `translate --refresh`
no longer re-ports an already ported function just because one it ports calls it.

### Belt drop-in (2026-11-16)

Checked in game on build `native-66f3981-793c65fede9b` ("porter trials"): 29 of 29 checks, and
`ai-test/create_belt_ends.py` 4 of 4, twice:

- An **item dropped on any segment** is taken in: a gold ingot dropped on the last segment
  rides the belt and the pulling funnel puts it in its chest. A segment that isn't the
  controller writes what lands on it into the controller's list (`entity_at`, `set_at` on
  `inventory.toInsert`), as `ItemHandlerBeltSegment.insertItem` does in Java.
- **Items keep Java's order** on the belt, and nothing drifts past the belt's far end any
  more: Create's `TransportedItemStack.compareTo` orders by position backwards (a lower
  position compares as greater), and the port had read it as the natural order, so an item
  put where others wait was listed behind them and the spacing rule pushed it back.
  Every ingot is accounted for in both phases.

What the porter gained: prompts quote the methods a function calls on what its code makes
(`new ItemHandlerBeltSegment(..)`, then `handler.insertItem(..)`) and on variables of a class
(`for (TransportedItemStack stack : items)` .. `stack.compareTo(..)`); they don't quote the Java
of a helper the module has its own function for (`checkForFunnels`), which left room for those;
they trim quoted methods by what a prompt is over Haiku's price line instead of halving them;
the landing note says each block takes in what lands on its own box, even where Java keeps it
in another block entity; and a block entity whose Java holds another of its own kind (a
controller) is told its own keys reach that one.

One hand edit, said in the code: the belt tick in the trial is Haiku's port that hands drops to
the controller, with its one sort comparison turned to Java's order. The belt tick is about 540
lines, at the edge of what Haiku writes in one answer: of three re-ports, one handed drops to the
controller, one kept Java's order, and one ran out of room. The general fix is to port what
lands on a block (`updateEntityAfterFallOn`, `entityInside`) as a function of its own beside the
tick, for every block that has one. Haiku for this round: about $0.29 (the night's total about
$2.98 of the $5 approved).

### On Horizon's mod API, build `f1bb66f` (2026-11-26)

Create's `api/` (the same 851 translated functions) compiled and linked against `f1bb66f`'s mod
SDK, built here from source, and ran on that build's dedicated server: 24 of the 29 checks pass
from its console (every check a console can run: the kinetics, diagonal cogwheels, the millstone,
press and mixer at work). The run found three things the API route lacked, now in the porter (the
mod's recipe types registered, Create's `{"item": ..}` ingredients read, a tick seeing the items
of the block entities it reaches and the blocks diagonally around it). Belts and anything a player
does weren't checked there. [API-TRIAL.md](API-TRIAL.md) has the details.

On the bundle (build `native-66f3981-793c65fede9b`), checked again the same day: 29 of 29, and the
belt tests 4 of 4.

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
