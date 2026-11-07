# Survey: 100 popular 26.3 mods through horizon-porter

On 2026-11-05 `tools/survey.py` took the most downloaded mods on Modrinth with a
Minecraft 26.3 version (libraries left out): the overall top 40, 40 popular
content-tagged mods, and 50 mods a server needs too (gameplay and content), 100 in
all once overlaps are counted once. For each one it downloaded the jar (NeoForge
first, else Fabric; checked against Modrinth's SHA-512), ran `horizon-porter port`,
ran the generated port's own tests, and compiled its Horizon helpers against the
API mock, and read its content (blocks, items, mobs) with Horizon's own content
loader (`horizon-mods`, built from the launcher's Horizon source): what a player's
game refuses to load. The jars stay local (`ai-test/`, not committed).

Nothing here was translated by a model or built into Horizon: it measures what
`port` does by itself. A port that builds is not a mod that works: what its code
does waits for translation, and a mod built on Java's renderer or internals (a
performance or shader mod, the verdict "Not portable yet" below) can't carry over at
all. Whether a port plays is what `horizon-porter check` says, in the game.

## Results

| | Mods |
| --- | --- |
| Port, Horizon's loader takes the content, the code compiles and passes its tests | 97 |
| Port as a content mod only (no code to port) | 3 |
| Fail | 0 |

At the start of the day, 26 of the first 90 failed or carried nothing over (see
"What the survey fixed").

What the report says before porting, from what each mod's Mixins change:

| Verdict | Mods | What it means |
| --- | --- | --- |
| No Mixins to judge | 25 | The mod works through hooks and its own code |
| Its Mixins reach Horizon | 17 | Every Mixin has a Horizon hook or a Horizon function to port into |
| Partly portable | 32 | Some Mixins draw, or patch the game's internals (performance mods) |
| Not portable yet | 26 | Most Mixins change how Minecraft draws: shaders, culling, entity models |

### Content

| Mod | Blocks | Items | Block entities | Mobs | Left to port |
| --- | --- | --- | --- | --- | --- |
| BetterNether | 678 | 88 | 4 | 6 | 5 |
| Biomes O' Plenty | 493 | 33 | 1 | 0 | 1 |
| Farmer's Delight (Refabricated) | 135 | 95 | 6 | 0 | 26 |
| BetterEnd | 100 | 11 | 6 | 6 | 19 |
| Traveler's Backpack | 60 | 20 | 1 | 0 | 32 |
| Waystones | 31 | 19 | 6 | 0 | 52 |
| Lootr | 15 | 0 | 0 | 0 | 9 |
| Tom's Simple Storage | 15 | 7 | 11 | 0 | 19 |
| Sophisticated Backpacks | 6 | 65 | 1 | 0 | 35 |
| Guard Villagers | 0 | 2 | 0 | 1 | 7 |

1,533 blocks, 343 items, 39 block entities and 13 mobs in all. Each mob copies the
vanilla mob Horizon offers that it is most like (hostile ones a zombie, other
creatures a wandering trader, villagers a villager) with its own health, speed,
armor and attack; mob effects keep their category and color (Farmer's Delight's
Comfort and Nourishment, BetterEnd's End Veil). Each block copies the vanilla
block it is most like (from its Java class, its properties, its blockstate file or
its name): Biomes O' Plenty's slabs, stairs, doors, fences, leaves, saplings, signs
and shelves copy theirs, 80 of its 493 blocks are left on stone, mostly blocks that
are stone-like.

"Left to port" counts the functions `port` leaves for translation (the median mod
has 6; FancyMenu, the most, 131): what the mod does, as opposed to what it adds.
With Claude Haiku a mod like Tom's Simple Storage costs about $0.05 to translate.

## A second batch: 50 more

The next 50 popular mods a server needs too: all 50 port (32 with code, 18 content
only), Horizon's loader takes their content, 6,228 blocks, 202 items, 11 block
entities and 59 mobs.

| Mod | Blocks | Items | Mobs |
| --- | --- | --- | --- |
| Rechiseled | 3,849 | 1 | 0 |
| Macaw's Roofs | 633 | 2 | 0 |
| Stoneworks | 572 | 0 | 0 |
| Macaw's Windows | 364 | 6 | 0 |
| Macaw's Doors | 301 | 13 | 0 |
| Macaw's Fences and Walls | 198 | 0 | 0 |
| Macaw's Bridges | 157 | 1 | 0 |
| Beautify | 65 | 0 | 0 |
| Naturalist | 35 | 111 | 48 |
| Friends and Foes | 24 | 19 | 10 |

Across both batches: 150 mods, 7,761 blocks, 545 items, 50 block entities and 72
mobs.

## What the survey fixed

- **Registrations through a mod's own wrappers** (Tom's Simple Storage, Biomes O'
  Plenty, Farmer's Delight, Waystones): a helper call's id wherever it is among its
  arguments, ids kept in constants, library registrars (`blocks.register(...)`,
  `RegUtils.regBlockEntity(...)`), and, failing all of those, the blockstate and item
  files. Biomes O' Plenty went from 0 blocks to 493.
- **Templates**: from blockstate properties, class name endings, the vanilla class a
  block is made of, its name, and 26.x's colored sets (`Blocks.CARPET.white()`).
- **Kotlin mods** (Vein Miner, Fzzy Config) decompile to Java.
- **Data-only mods** (Dungeons and Taverns, Towns and Towers) port as content mods.
- **Version numbers** that aren't semver (`19.21.0.1247`) no longer break a port.
- **Content Horizon's loader refused**: a cable's `connects_to` without its `#`, a
  constant naming a block and its item (`id("cabbages", "cabbage_seeds")`) read as
  one id, and files an earlier `port --force` left behind.
- **A description with line breaks** made an invalid `horizon-mod.toml` (Advanced
  Netherite, Villagers Plus), and two statics with one Rust name (AzureLib's
  `MAX_OVERHANG` and `maxOverhang`) clashed.
- **Generated code** that warned or clashed: text-only configs, config keys that are
  sentences, a message named `Payload`, two messages on one channel.
- **The report** calls performance mods partly portable: their Mixins patch the
  game's internals, and Horizon is its own engine.
- **Mobs and mob effects** port as Horizon's entity types and effects (they didn't
  before), and Farmer's Delight's ids kept in constants, `reg...` registrars, food
  and 26.x's colored blocks are read.

## What to target next

1. **Translation, automatically.** 989 functions are left across the 100 mods. The
   app translates them for a few cents, but only with a Claude API key or a local
   model; making that step need no setup is the largest remaining step to
   instant porting.
2. **Mobs' looks and behaviour.** A mod's mob now exists, with its stats, but it
   looks and acts like the vanilla mob it copies until its model, renderer and AI
   are ported (`mob_submit`, goals). Projectiles aren't ported yet.
3. **Rendering mods** (26 "not portable yet") need Horizon's renderer to open up:
   shaders, culling, entity models. That is work for Horizon, not the porter.
4. **Checking content in game.** The survey proves ports build; whether every block
   looks and behaves right needs playing them, starting with Biomes O' Plenty and
   Farmer's Delight.

Run it again with `python tools/survey.py --count 40` (add `--server` for gameplay
mods, `--slugs a,b` for given ones, `--skip-download` to reuse the jars).
