# Architecture

horizon-porter helps a mod developer port a Minecraft Java Edition mod (Fabric,
NeoForge or Forge, for Minecraft 26.3) to a [Horizon](https://github.com/horizon-rust/horizon)
native mod: a [horizon-mixins](https://github.com/horizon-rust/horizon-mixins) bundle
(`.hnmod`) plus a content mod (`.hmod`) for its data and assets.

It is a **porting assistant, not a magic converter**. It does everything that can be
done mechanically, uses AI for the parts that need judgment, puts every result through
the Rust compiler, and tells you exactly what is left for a human.

## Design principles

1. **Source first.** The input is the mod's own source tree (a Gradle project), which a
   mod developer has. Jars are only *scanned* for reports. Porting from decompiled code
   loses names, comments and generics, and raises licensing problems.
2. **Translate against a stable API, not Horizon's internals.** Generated code targets
   **modkit** (below), a Fabric/NeoForge-like API built on Horizon. Horizon changes
   every day; modkit absorbs that, so ported mods keep building.
3. **Deterministic where possible, AI where necessary, compiler always.** Registration,
   metadata, Mixin annotations and API calls are mapped by rules. Method bodies that need
   judgment go to an LLM with the right context. Nothing generated is accepted until
   `cargo check` passes.
4. **Incremental and resumable.** Every class and Mixin has a status in
   `porter-state.json`. Re-running never overwrites code a human has edited (tracked
   by content hash); it reports a conflict instead.
5. **Honest reports.** Every class, Mixin, registry entry and asset gets a verdict:
   ported, partial, stubbed or blocked (with the reason, such as a missing Horizon hook).
6. **Port libraries once.** Registrate, Flywheel, Ponder, Architectury, Cloth Config and
   similar are ported once as shared shims, so mods that depend on them don't each
   redo that work.

## Pipeline

```
mod source (Gradle project)
   │
   ▼
[scan]      parse Java, loader metadata, Mixin configs, assets, data
   │        resolve every reference: Minecraft / loader API / library / own code
   ▼
[report]    coverage, mapping per symbol, blocked items, effort estimate
   │
   ▼
[port]      content  → .hmod (data + assets)
   │        skeleton → bundle layout, horizon-mixin.toml, compatibility.json
   │        mixins   → horizon-mixins injections + helper stubs
   │        classes  → Rust modules: rules first, then AI per method
   ▼
[check]     cargo check through horizon-mods' native.py → errors fed back
   │        to the translator (compile/fix loop, bounded attempts)
   ▼
[verify]    unit tests; later, differential scenarios: the same world actions
            in Java (with the mod) and in Horizon (with the port), compared
```

## Components

| Crate | Job |
| --- | --- |
| `porter-cli` | The `horizon-porter` command: `scan`, `report`, `port`, `check`, `status`. |
| `porter-model` | Shared types: the project model, symbols, verdicts, the state file. |
| `porter-java` | Reads a mod: Java source (tree-sitter-java), `fabric.mod.json`, `neoforge.mods.toml` / `mods.toml`, `*.mixins.json`, assets and data. Reads class files for libraries without source. |
| `porter-map` | The mapping database: Horizon's hook table (`data/horizon-hooks.json`, made by `tools/horizon_hooks.py` from Horizon's source: each hook's signature, contract, version and the vanilla methods and events its doc names), Mixin targets and loader events → hooks, and a lookup of the Horizon functions whose comments name a vanilla method (a light port ledger over the launcher's Horizon checkout). |
| `porter-mixin` | Turns `@Mixin` classes into `[[injection]]` entries and helper files. |
| `porter-content` | Blocks, items, data and assets → a content mod (`content/`, packed as `.hmod`): vanilla templates, properties and declared state properties read from the Java code and blockstate files. |
| `porter-translate` | The native mod (`bundle`), block entities, machines (`machine`: menus, screens and recipes), network messages and config (`netconf`), the mod's own screens (`screens`), the Java statics carried into `runtime.rs` (`statics`), and the AI pass (`ai`): `translate` sends each function left to port to a model (Claude through the Messages API, Ollama, Claude Code, or a command), splices the answer in, and checks `runtime.rs` with `cargo test`, feeding errors back. |
| `porter-verify` | Runs `native.py check`, parses `cargo` diagnostics, runs tests, and later the differential scenarios. |
| `modkit/` | The **runtime** that ported mods depend on: a horizon-mixins bundle with its own crates (horizon-mixins 0.6 `[[crate]]`). See below. |

## modkit: the runtime layer

Java mods sit on a large API: Minecraft's classes plus Fabric API or NeoForge. Horizon's
native hook points today cover a HUD, keys, commands, settings, ticks and a recipe
panel. That is enough for client mods, but not for a mod that adds blocks with
behaviour. modkit fills that gap and gives the translator one stable target:

- **Registries:** blocks, items, block entities, entities, menus, recipe types and
  serializers, sounds, particles, creative tabs.
- **Block entities:** per-position state, ticking, saving to and loading from NBT (serde),
  client sync.
- **Events:** the common NeoForge / Fabric events as modkit hooks.
- **Networking:** typed client↔server packets (Horizon's single-player server runs on
  its own thread, so even single-player mods need this).
- **Menus and screens**, **config**, **capabilities / transfer** (items, fluids, energy).
- **Rendering:** block entity renderers, custom baked models, item renderers, and later
  instanced rendering (what Flywheel provides in Java).

modkit is built on Horizon's named hook points wherever they exist. Where they don't,
it hooks Horizon's own functions (pinned to a Nightly commit), and each of those is a
candidate **upstream hook point** to propose to Horizon. The scan report lists the hooks
a mod needs that Horizon lacks, so the requests to Horizon come with evidence.

## Mapping Java to Rust

| Java | Rust in a ported mod |
| --- | --- |
| `class ShaftBlock extends RotatedPillarKineticBlock` | a struct plus `impl ModBlock` / trait impls; deep hierarchies become composition and traits with default methods |
| `BlockEntity` fields | a `#[derive(Serialize, Deserialize)]` state struct owned by modkit's block entity storage |
| static registry objects | typed registry handles (`BlockId<ShaftBlock>`) |
| `null` | `Option` |
| event bus subscribers | modkit hook functions |
| `@Inject(at = @At("HEAD"))` | horizon-mixins `HEAD` (cancellable when the Java one is) |
| `@Inject(at = @At("RETURN"))` | `RETURN` |
| `@Inject(at = @At("INVOKE"))` | `BEFORE_CALL` / `AFTER_CALL` |
| `@ModifyArg`, `@ModifyArgs`, `@ModifyVariable`, `@ModifyConstant` | the same-named kinds |
| `@Overwrite` | `OVERWRITE` |
| MixinExtras `@WrapOperation`, `@ModifyExpressionValue` | `WRAP_OPERATION`, `MODIFY_EXPRESSION_VALUE` |
| `@Accessor`, `@Invoker` | `GET` / `SET` / `INVOKE` bridges |
| `@Unique` fields | horizon-mixins namespaced instance state |
| `@Redirect`, `TAIL`, `FIELD` / `NEW` targets, `@Shadow`, interface mixins | **not yet in horizon-mixins**: reported as blocked, with the closest workaround suggested |

The Mixin rows follow horizon-mixins' own
[SpongePowered feature map](https://github.com/horizon-rust/horizon-mixins/blob/main/docs/spongepowered-feature-gaps.md).
A Java Mixin targets a Java method, but horizon-mixins targets a Rust function. The
port ledger says which Rust function does the job of a given Java method. Where the
code has a different shape, the injection point needs a human or AI decision, and the
report says so.

## Trust and safety

Native mods are not sandboxed. horizon-porter never builds or runs generated code
without the same explicit trust flag Horizon's tools require, and generated code is
meant to be reviewed: the output is plain, commented Rust, and the report links every
generated function to the Java method it came from.
