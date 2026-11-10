# Using horizon-porter

The full reference for the command line. The [README](../README.md) has the short version.

## Installing the command line

Download the `horizon-porter` archive for your system from the
[latest release](https://github.com/kevinbonomi2-bit/horizon-porter/releases/latest),
extract it, and run `horizon-porter` from a terminal in that folder:

| System | File |
| --- | --- |
| Windows | `horizon-porter-<version>-x86_64-pc-windows-msvc.zip` |
| macOS (Apple Silicon) | `horizon-porter-<version>-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `horizon-porter-<version>-x86_64-apple-darwin.tar.gz` |
| Linux | `horizon-porter-<version>-x86_64-unknown-linux-gnu.tar.gz` |

The builds are not signed yet. On macOS, allow the first run in System Settings →
Privacy & Security, or run `xattr -d com.apple.quarantine horizon-porter` once.

With Rust installed, you can instead build and install it from source:

```
cargo install --git https://github.com/kevinbonomi2-bit/horizon-porter porter-cli
```

## Commands

```
horizon-porter scan path/to/your-mod              # one-screen summary
horizon-porter scan path/to/your-mod --json scan.json
horizon-porter report path/to/your-mod            # the full breakdown
horizon-porter report path/to/your-mod --all      # every Mixin member, not only problems
horizon-porter report path/to/your-mod --html report.html   # a page to open or share
```

`--html` writes the whole report as a single self-contained page (no scripts, nothing
loaded from elsewhere). Open it in a browser, attach it to an issue, or post it.

### Porting a mod

```
horizon-porter port path/to/your-mod --out my-mod-horizon
horizon-porter port some-mod-1.0.jar --out some-mod-horizon      # a jar works too
```

A jar is decompiled into a source tree first, with
[Vineflower](https://github.com/Vineflower/vineflower) (Apache-2.0) on the Java
runtime the Horizon launcher keeps (else `JAVA_HOME` or `java`). Vineflower is
downloaded once from Maven Central and checked against its published SHA-256. The
mod's own modules nested in the jar (jar-in-jar, `c2me-base` inside `c2me`) are
decompiled with it, and bundled libraries left out. Each jar is decompiled once and
kept, so porting it again is instant. Port only mods whose licence allows it.

`port` writes a Horizon native mod (a [horizon-mixins](https://github.com/horizon-rust/horizon-mixins)
bundle) for a client mod:

- **Wired for you:** the mod's row in Options > Mods, its key bindings (with their
  default keys and English labels, rebindable by players), its client commands, a HUD
  box, the client tick, the mouse wheel and a settings page. Each hook calls a function
  in `runtime.rs`.
- **Left for you:** the Java logic itself. Each stub in `runtime.rs` names the Java
  class to port into it, and `PORTING.md` lists every class, the Mixins with their
  verdicts, and the other Horizon hooks the mod needs (screens, tooltips, field of view,
  chat...), mapped from the Fabric and NeoForge events it uses.

The stubs get the player (position, yaw, pitch, dimension, and `facing()` as Java's
`Direction.fromYRot`), so ported logic can use them. `cargo test` works in the
generated folder straight away. `port` never overwrites a folder that has files unless
you pass `--force`, and then only replaces its own files.

### Porting blocks, items and block entities

The same `port` command carries a mod's content over. For a mod that registers blocks
and items it also writes `content/`, a Horizon content mod:

- **`horizon-mod.toml`** and the Java mod's own assets and data (models, textures,
  lang, recipes, loot tables, tags), in the namespaces Horizon accepts.
- **A file per block**, copying a vanilla block (its template) and changing what the
  Java code sets: `ofFullCopy(Blocks.IRON_BLOCK)` gives the template, `strength`,
  `lightLevel`, `sound` and `requiresCorrectToolForDrops` give its properties, and
  Registrate's builder chain is read the same way. A block with no vanilla block named
  gets one from its class (`SlabBlock`, `DoorBlock`...) or from the state properties
  its blockstate file uses (an `axis` block copies a log), and declares the state
  properties its template lacks so its models still match.
- **A file per item**, from `Item.Properties` (`stacksTo`, `rarity`, tools, food).

Blocks and items whose **Java acts in code** get a function each in `runtime.rs` to
port into, called from Horizon's hooks: a block placed its own way
(`getStateForPlacement`), used with a right click (`useItemOn`, `useWithoutItem`) or
reshaped by its neighbours (`updateShape`); an item used on a block (`useOn`) or in the
air (`use`), which can change blocks and use up or wear the stack in hand. A click's
code knows which way the player faces and whether they're in creative, and reaches the
block's block entity: its saved values and its item slots. Until one is ported, its
template's own behaviour stands.

A block entity that opens a **vanilla menu** over its own slots (a cabinet's
`ChestMenu.threeRows`, a hopper's or a dispenser's) is declared to Horizon as a container
of that size, so it opens the chest's screen and hoppers reach it.

Ported block code asks the world what Java asks it: a block and its state, whether it is
in a tag (`world.in_tag(pos, "minecraft:dirt")`; where Horizon gives no tag query, from a
table of the vanilla tags the mod's Java names and the mod's own), the water and the light
at a spot, whether a block's face is sturdy, and whether redstone powers it. A click's
code can also show an action-bar line or a chat line and break a block with its drops, and a placement
knows the item in the placer's hand. A block that always holds water in Java (rice,
a `LiquidBlockContainer`) keeps its water.

It can spawn entities too (`world.summon`, a baby with `summon_baby`, something thrown
with `launch`): from a click or an item at once, from a block's tick once the level's
tick is over (an egg hatching). A use knows where the player stands, their eyes and the
way they look. A block whose Java `updateShape` returns air breaks, with its drops, and
one that schedules a tick there gets its tick.

The mod's **entity types** come over as content: a mob copies the vanilla mob it is most
like (a zombie, a villager, or a neutral wandering trader) with its own attributes, and a
thrown item (`ThrowableItemProjectile`) becomes Horizon's thrown type, drawn as its item
and thrown by using it. Other projectiles and vehicles are left out and listed in
PORTING.md.

Each **block entity** becomes a state struct in `runtime.rs`, saved with its block
under the keys `saveAdditional` writes, plus a `tick_<name>` stub for its Java tick.
Horizon's `native_block_entity` hooks give the blocks their block entity and run the
tick on the server, saving what changed. Port the tick's body and the block works:
`/data get block <x y z>` shows its saved values. `PORTING.md` lists every block's
template and why, and anything left out.

A block entity that opens a menu is a **machine** (a crusher, a furnace of the mod's
own). Its Java menu is read for its slots (`addSlot(new Slot(container, 0, 56, 35))`),
which of them players only take from, where the player's inventory starts and how many
data slots it syncs; its screen for its size and texture. The port gets a menu type in
`content/`, the block's slots and menu (`native_block_entity_container`, with hoppers
putting in from above and the sides and taking results from below), and its screen
laid out as the Java one, drawn with the mod's own texture (`native_screen`,
`native_screen_draw`). The machine's block copies a furnace or a barrel, which open a
menu. Its tick gets a `Machine`: the slots, the data slots its screen shows, and the
mod's recipes of its own types, read from their JSON files with ingredients resolved
to items (`machine.recipe_for("grind:crushing", 0)`). The screen's progress arrows are a
`draw_<name>` function to port, like the tick.

A block entity's **fluid tanks and energy buffer** (NeoForge's handlers, `FluidTank`,
`EnergyStorage`) are declared to Horizon, so pipes and cables reach them, and its tick
gets them to fill, drain and spend (`machine.tanks[0]`, `machine.use_energy(20)`). A
tick moves items to and from the blocks beside it as a hopper does
(`machine.push(1, Side::Down, 64)`, `machine.pull(...)`): a machine's auto-output.

A mod's **network messages** (`CustomPacketPayload` records) become Rust structs with
their channel and fields, sent and received through Horizon's `native_net_*` hooks: the
port decodes each message and calls its handler, which you (or the translation) port
from the Java one. Its **config** (NeoForge's `ModConfigSpec`, or a class of plain fields
as Cloth/AutoConfig, OmegaConfig and MidnightLib mods keep theirs) becomes a `Config` kept in
Horizon's mod settings, with a page in Options > Mods that toggles, cycles and steps each value.

A **block entity renderer** becomes a `render_<name>` function that draws with a pose
stack (push, pop, translate, rotate, scale) and block models, cubes and items, animated
with the frame's time: Horizon draws it each frame (`native_block_entity_submit`) from
the state the server sends. A **screen of the mod's own** (a guidebook, a menu opened by
a key) becomes a struct with `draw_`, `click_`, `key_` and `scroll_` functions, shown
over the world with Horizon's mod screens.

The report leads with whether the mod can be ported **today**. A Mixin can hook in the
way horizon-mixins supports and still change something Horizon can't do yet: Mixins
that change how the game or its screens are drawn reach Minecraft's renderer, which
Horizon replaces with its own. A mod made mostly of those (a shader, a 3D GUI) is
reported as "Not portable yet", and the app won't offer to pay for translating it.

A mod that is only content (no client hooks, no block entities) gets `content/` alone:
nothing to build or trust.

### Porting Mixins and events

Horizon has hundreds of hook points (the mod's `native_*` functions), and each one's
doc names the vanilla method or loader event it stands for. horizon-porter ships a
table of them, read from Horizon's source, so `port`:

- **maps each Mixin to the hook that does its job:** `Gui.render` becomes the HUD,
  `GameRenderer.getFov` becomes `native_compute_fov`, `Player.canEat` becomes
  `native_can_eat`, and NeoForge events (`LivingEvent.LivingJumpEvent`) map the same
  way. Hook points hold across Horizon versions; a Mixin into Horizon's own functions
  would pin the mod to one Horizon commit.
- **gives every hook it needs a helper** with Horizon's own parameters, the Java code
  to port named at the top, and the right hook version in `horizon-native.json`.
- **points the other Mixins at the Horizon functions** whose comments name their
  target, read from the Horizon source your launcher keeps (or `HORIZON_SOURCE`), and
  says so when Horizon doesn't port that method yet.

`PORTING.md`'s Mixins table says where each Mixin went.

### Translating the rest with AI

`port` wires everything and leaves each function's logic marked TODO.
`translate` ports those functions with an AI model, one at a time:

```
horizon-porter translate my-mod-horizon --dry-run   # what it would do, and what it costs
horizon-porter translate my-mod-horizon             # Claude Haiku 5.5 (ANTHROPIC_API_KEY)
horizon-porter translate my-mod-horizon --ai ollama --model <a local model>   # free
```

Each function goes to the model with the Java it replaces, the parts of the mod's
Rust module it can use (the Java mod's static fields are there too, as Rust), and for
hook helpers the game functions Horizon gives hook code. The answer replaces the
function. Functions in `runtime.rs` are compiled and tested with `cargo test`, and
an answer that fails goes back to the model with the compiler's message (three tries,
then the function is left as it was). Hook helpers compile inside Horizon, so
`build` checks them. Each ported function says so above it: read it before you trust
it.

It only touches functions that still carry their TODO, so it never overwrites one you
wrote, and it saves each result as it comes (`.porter/translate.json`), so it can stop
and resume. `port --force` keeps ported functions too.

**Porting again with a newer horizon-porter.** Each version gives ported code more to work
with (a block's tags, the water and light at a spot, dropped items, redstone). A function
ported before keeps its old port: `port --force` marks the ones it now describes
differently (`Outdated:` above them) and says how many could be ported better.

```
horizon-porter translate my-mod-horizon --refresh --dry-run   # which ones, and the cost
horizon-porter translate my-mod-horizon --refresh             # port them again
```

`--refresh` ports those again from the Java alone, and also the functions whose port said
something was out of reach (`// Missing: ...`) that this version can now reach. One that
fails keeps its old port. The app does this when you port a mod again.

**What didn't come over.** After translating, `TRANSLATION-NOTES.md` in the port folder lists
each ported function that couldn't do part of what its Java does, and what that part is.

**What it costs.** `--dry-run` prints an estimate before anything is spent, and more
than five functions need `--yes`. Claude Haiku 5.5 is the default: about $2 for
Farmer's Delight's 117 functions.
`--model claude-sonnet-5-5` or `claude-opus-5-5` costs more (Create about $7.80 on
Opus 5.5) and is for what Haiku gets wrong. `--ai ollama --model <name>` runs a model
on your own computer with [Ollama](https://ollama.com): free, and nothing leaves the
computer. `--ai claude-code` uses your `claude` command, and `--ai "<command>"` any
program that reads the prompt on stdin.

### Adding it to your Horizon

```
horizon-porter install my-mod-horizon                          # into the instance the launcher's Play starts
horizon-porter install my-mod-horizon --instance "My modpack" --build --trust-native-code
```

`install` packs the mod and adds it to one of your instances beside its other mods: the
content mod (`<id>.hmod`) in its `horizon-mods/`, the code (`<id>.hnmod`) in its
`horizon-native-mods/`, turned on in Options > Mods. Every native mod in an instance is
built into one game, so a ported mod plays alongside other mods and modpacks. Build the
instance with `--build`, or from the launcher's Mods page, which shows "Needs rebuild".

A translated function the game's build refuses (one that calls something Horizon
doesn't have where it runs) doesn't stop the mod: `install --build` and the app put that
function back to its stub, marked to port again, and build once more. The mod plays
without it, and the build says which ones it put back.

### As a mod on Horizon's new mod API (experimental)

```
horizon-porter port my-mod.jar --out my-mod-horizon --api
```

Horizon's mod API (`mod_api`, in the [arismoko/horizon](https://github.com/arismoko/horizon)
fork for now) replaces horizon-mixins bundles with typed hooks and Parts. `--api` writes the mod
that way too, in `api/`, beside the bundle: `horizon-mod.toml` with `[native]` pins to the Horizon
commit the porter was checked against, `Cargo.toml`, the content's `assets/` and `data/`,
`src/runtime.rs` (the bundle's, with what `translate` ported in it; `translate` refreshes it) and
`src/lib.rs`, the glue:

- a `BlockPart` for placement, clicks, shape updates, ticks, survival and comparators, and an
  `ItemPart` for item use and tooltips;
- a `BlockEntityPart` for each block entity: its saved state, its tick (slots, data, recipes, the
  blocks beside it, sounds, particles, drops), its container and menu, its screen and shift-clicks,
  its renderer, what clients get of it, and what it drops when broken.

Build it with that fork's launcher (`horizon-launcher native-build my-mod-horizon/api
--trust-native-code`). [docs/API-TRIAL.md](API-TRIAL.md) has what was tried, what plays, and what's
next.

### Trying it in the game

```
horizon-porter check my-mod-horizon --instance "My modpack"
```

`check` plays the port in the instance it is installed and built in, as the app does
after a build: it opens a creative test world (`porter-check`, never one of yours), on a
control socket of its own (a game you have open is left alone), places each of the
mod's blocks and reads it back, right-clicks each block whose Java acts on a click
(empty-handed, then holding what its ported code asks for: an item it compares the held
one to, the mod's tools, shears, the block's own item), places by hand, with its item, each
block whose placement is ported code (`/setblock` doesn't run it), and reads the game's log. Faults (a block the game refuses or can't draw, content Horizon
didn't load, a game that stops) fail it; notes (a click that changed nothing, a model or
texture drawn as Minecraft's missing one) are for you to look at.

It uses no AI and costs nothing: it runs the game you already have, on your computer,
for a few minutes (a few seconds a block).

Does a port work before its logic is translated? Its content does: blocks, items,
models, textures, recipes, loot and tags load and play as they are. What the Java code
*does* (a machine's tick, a HUD, keys, what a Mixin changes) waits in stubs until
`translate` or you port it: the mod loads and nothing breaks, it just doesn't act yet.

### Building, playing and sharing it

```
horizon-porter build my-mod-horizon --trust-native-code --play
horizon-porter play my-mod-horizon        # play the last build again, no rebuild
horizon-porter package my-mod-horizon
```

`build` finds your Horizon launcher (or takes `--launcher PATH`) and runs its
`native-build` into an instance of its own, **<mod id> port**, created the first time,
so your other worlds never run the mod. It builds on the Horizon that instance plays:
the generated `horizon-native.json` asks for the hook points version it needs instead
of pinning one Horizon commit. `--trust-native-code` is required every time, because
native mods run unsandboxed: read the code first. `--play` starts the game with
`content/` loaded as a development mod. A content mod alone needs no build or trust:
`build --play` just plays it.

`package` writes `<id>-<version>.hnmod`, the file Horizon's mod registry takes, packed
the way Horizon's own mods are, and `<id>-<version>.hmod` from `content/`. Players put
the `.hmod` in their game folder's `horizon-mods/` and turn it on in Options > Mods.

The path is your mod's source folder (the Gradle project; multi-loader projects work)
or a built `.jar`. `scan` and `report` read a jar's classes as they are, quickly;
`port` decompiles it first (below), and a decompiled jar gives the full picture too.

`scan` tells you what the mod is made of: classes, Mixins and their verdicts, registry
entries, resources, and what it depends on (Minecraft, the loader API, other mods).
`report` adds the details: each Mixin that needs work and why, the Minecraft and loader
classes the mod uses most, the Minecraft classes it extends, the libraries it needs
first, and any direct OpenGL use. [docs/CREATE.md](CREATE.md#what-the-scanner-found)
shows what it found in Create.
