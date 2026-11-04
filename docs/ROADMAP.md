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

- **M0, first version (2026-11-04):** `scan` and `report` work on source trees
  (Fabric, NeoForge, Forge, multi-loader) and jars. They are tested on two fixture mods
  and a generated jar, and run on Create's full source in about 2 seconds
  ([results](CREATE.md#what-the-scanner-found)). Still to do for M0: hand-check reports
  for more popular mods, count registrations made through helper methods, and read
  Mixin details from jars.

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
