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
