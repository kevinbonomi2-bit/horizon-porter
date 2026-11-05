# Roadmap

Each milestone ends with a real mod ported end to end. A milestone is done only when
that mod builds with Horizon's tools and behaves like the Java original in the
scenarios we check.

| # | Milestone | What it unlocks | Proof |
| --- | --- | --- | --- |
| M0 | **Scan and report** | `horizon-porter scan` on any mod's source or jar: classes, Mixins, registries, assets, and a verdict per item. Useful on day one, even before anything is ported. | Reports for 10 popular mods, checked by hand |
| M1 | **Client mods** | Skeleton generation, `compatibility.json`, mappings to Horizon's existing hook points (HUD, keys, commands, settings, zoom). | A small Fabric HUD / keybind mod ported and running |
| M2 | **Content + blocks with behaviour** | `.hmod` carry-over; modkit registries, block entities, ticking, NBT saves. Needs the first upstream hook points in Horizon. | A small tech mod: one machine block that ticks and keeps its state |
| M3 | **Mixins** | Mixin translation via the port ledger, for the kinds horizon-mixins supports; blocked ones reported. | A mod that relies on several `@Inject` / `@ModifyArg` Mixins |
| M4 | **AI translation loop** | Per-method AI translation with the compile/fix loop, resumable state, conflict-safe regeneration. | A mid-size mod (30–80 classes) mostly ported automatically |
| M5 | **Menus, networking, config, recipes** | Container screens, client↔server packets, config files, custom recipe types. | A mod with a machine GUI and a custom recipe type |
| M6 | **Rendering** | Block entity renderers, baked / connected-texture models, item renderers, then instanced rendering (Flywheel's role). | Animated rotating parts drawn correctly |
| M7 | **Large mods** | Library shims (Registrate, Flywheel, Ponder), fluids and transfer APIs, moving structures. | Create's kinetics and basic machines (see [CREATE.md](CREATE.md)) |

## Status

- **M0, done (2026-11-04):** `scan` and `report` work on source trees (Fabric,
  NeoForge, Forge, multi-loader) and jars, Mixin details included. Registrations are
  found through Registrate, DeferredRegister, `Registry.register`, multi-loader
  `BiConsumer<T, ResourceLocation>` callbacks, and helper methods that pass an id on
  (helpers of helpers too). Reports were checked by hand on six real mods:

  | Mod | Kind | Classes | Mixin members (supported / partial / blocked / not needed) | Registrations | Scan time |
  | --- | --- | --: | --- | --: | --: |
  | Create 6.0.11 | NeoForge tech | 2,011 | 108 / 5 / 2 / 29 | 570 | 2 s |
  | Applied Energistics 2 | NeoForge tech | 1,409 | 15 / 5 / 2 / 12 | 308 | 9 s |
  | Botania | Fabric + Forge magic | 1,098 | 195 / 2 / 7 / 42 | 1,139 | 7 s |
  | Sodium | Fabric + NeoForge performance | 647 | 264 / 12 / 34 / 109 | 0 | 4 s |
  | Farmer's Delight | NeoForge content | 237 | 16 / 1 / 0 / 3 | 379 | 2 s |
  | Mod Menu | Fabric client | 52 | 4 / 0 / 3 / 1 | 0 | 0.3 s |

  What the six have in common: Mixins that add an interface to a Minecraft class
  (`implements` on the Mixin) are the most common blocked case (28 of Sodium's 34), so
  that is the first horizon-mixins feature worth proposing. Known gaps: registrations
  whose id is a constant (`AEBlockIds.QUARTZ`) are counted without their id, and Java
  type annotations on varargs (`String @NotNull ...`) are reported as unreadable lines.

- **M1, done (2026-11-04):** `port` generates a native mod bundle for client mods,
  with eight hook points wired (`mods`, `key`, `command`, `hud`, `tick`, `wheel`,
  `settings`, `settings_click`) to stubs that get the player, and 38 Fabric and
  NeoForge client APIs mapped to Horizon hooks for the `PORTING.md` checklist. The
  bundle declares hook points version 1 (`horizon-native.json`), so it builds on the
  player's Horizon. `build` compiles it with the Horizon launcher into its own instance
  and `package` writes a `.hnmod`. **Tested live:** Compass HUD, ported and its logic
  filled in, built with the launcher and played in Horizon (keys, HUD, command, the
  Options > Mods row). The helpers are also type-checked in CI against a mock of
  Horizon's hook API.
- **M2, done (2026-11-04):** `port` also writes a content mod (`content/`): the Java
  mod's assets and data, a block file per block (template from `ofFullCopy` /
  `Properties.copy`, the block's class or its blockstate's properties; hardness,
  resistance, light, sounds and tool from its properties, Registrate chains included;
  declared state properties where the template lacks them) and an item file per item.
  Block entities become state structs saved with their block under the Java keys, with
  tick stubs, wired to `native_block_entity`, `native_block_entity_ticks` and
  `native_block_entity_tick` (hook points version 129). `build --play` loads `content/`
  as a dev mod, and `package` also writes the `.hmod`. On Create: 190 blocks, 84 items,
  7,793 resource files and 114 block entity states, whose generated code compiles and
  passes its tests. **Tested live:** the demo mod's crank, its tick filled in, built
  with the launcher and played in Horizon: the block places with its properties, and
  `/data get block` shows its saved `Speed` rising each tick.
- **M3, done (2026-11-05):** Mixins map to Horizon hook points rather than to
  Horizon's internals. `tools/horizon_hooks.py` reads all 386 hooks from Horizon's
  source (signature, contract, version, and the vanilla methods and events each doc
  names) into a table the porter ships. `port` gives every hook a mod needs a helper
  with Horizon's own bindings (all of them compile against a mock of the hook API in
  CI), maps Mixin targets and NeoForge events to hooks (by citation, a table of client
  targets and by name), sets the bundle's hook version from what it uses, and points
  the remaining Mixins at the Horizon functions whose comments name their target.
  On real mods: Botania's Mixins reach 22 hooks and 25 Horizon functions, Create's 3
  and 15; most of the rest target code Horizon doesn't port by name yet (Sodium's
  renderer Mixins above all). **Tested live:** the Steady View fixture's six Mixins
  ported (four to hooks), two helpers and the HUD filled in, built with the launcher
  and played in Horizon: the view stays steady when sprinting and the HUD counts
  pickups.
- **M4, done (2026-11-05):** `translate` ports the functions `port` leaves, with an
  AI model and the compiler: each function gets the Java it replaces, the parts of the
  mod's module it can use and, for hook helpers, Horizon's game functions (174, now in
  the hook table); `runtime.rs` answers are compiled and tested, and failures go back
  with the compiler's message. It never touches a function without its TODO, resumes
  from `.porter/translate.json`, and `port --force` keeps ported functions. The Java
  mod's static fields are carried into `runtime.rs` as Rust (constants, atomics,
  mutexes) so ported functions share state. Claude Haiku 5.5 is the default (Create's
  whole pass is about $0.20), with Sonnet and Opus by `--model`, a free local model
  with `--ai ollama`, Claude Code, or any command. **Tested live:** Haiku 5.5 ported
  Steady View's HUD, pickup counter and fixed field of view and the demo crank's tick,
  each on its first try, for about a cent; both built with the launcher and played in
  Horizon: the HUD counts pickups, the view stays steady, and the crank's `Speed` rises.
- **Desktop app (2026-11-04):** Horizon Porter (`app/`, Tauri) wraps scan, report and
  port in a drag-and-drop window, with installers for Windows, macOS and Linux in each
  release. Designed with the unslop and oil-ui skills (see the README).
- **For players (v0.6.0, 2026-11-05):** a mod's jar is enough: it is decompiled with
  Vineflower (jar-in-jar modules included) and ported like source. The app has one
  button, **Make it work in Horizon**: one summary (what carries over, who translates
  its code and the cost, which instance it goes in) and one yes, then it ports,
  translates, adds the mod to the player's own instance beside their other mods, and
  builds it. It says plainly when code won't be translated, and offers Translate now
  after. The Claude API key is kept in the system's credential store. `install` does
  the same from the command line. **Tested live** in the app: Demo Machines added to an
  instance and played; C2ME's jar (21 modules, 513 classes) decompiled and ported.
- **Also found:** Horizon now has 427 versioned hook points (`native-hooks.json`) and
  159 helpers, far beyond the documented client set: blocks, block entities (with
  drawing helpers), entities, loot, players, screens and more. That makes M2 (content
  with behaviour) closer than this plan first assumed.

## Working with Horizon

The tool depends on Horizon gaining hook points for things mods need (block behaviour,
block entities, entities, menus, rendering). The plan is to work *with* the Horizon
maintainers rather than around them:

- M0's reports collect which hooks real mods need, which is evidence for each request.
- Each needed hook is proposed as a small Horizon pull request that follows their
  contributing rules (vanilla first, small PRs, `tools/pr_ready.py`).
- Until a hook lands, modkit uses a pinned-commit hook into Horizon's own functions, and
  the report says that the mod is pinned.

## Out of scope (for now)

- Running Java mod jars inside Horizon (a JVM bridge). It is a different project with
  different trade-offs.
- Porting mods you don't have the rights to port. horizon-porter reads the mod's
  licence and warns when it doesn't allow derivative works.
- Mods for Minecraft versions other than 26.3: update the mod to 26.3 in Java first.
