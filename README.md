# horizon-porter

A tool for mod developers to port Minecraft Java Edition mods (Fabric, NeoForge,
Forge; Minecraft 26.3) to [Horizon](https://github.com/horizon-rust/horizon), the
Rust rewrite of Minecraft, as native Horizon mods.

> **Status: early.** Scanning and reports work for any mod. Porting works for client
> mods (keys, commands, HUD, tick, settings), with the logic left to port by hand, and
> a ported mod builds and plays in real Horizon through your Horizon launcher. The plan
> is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/ROADMAP.md](docs/ROADMAP.md).
> Not affiliated with Horizon, Mojang or Microsoft.

## Horizon Porter, the desktop app

Drop a mod's source folder or `.jar` on the window. Horizon Porter reads it and shows
the report: every Mixin member as a slot colored by its verdict, what the mod builds
on, its registrations and resources. Then one button, **Make it work in Horizon**, shows
a short summary: what carries over, who translates its code and what that costs, and
which of your Horizon instances it goes in (the one the launcher's Play starts, unless
you pick another). Say yes once, and it ports the mod, translates its code, adds it to
that instance beside your other mods and modpacks, turned on in Options > Mods, builds
the instance's native mods together, and ends on **Play**. A mod that is only content
(blocks, items, recipes) needs no translation or build: it is just added.

The **⋯** menu has the rest: **Save the report**, **Port to a folder** (for mod
developers, who read and edit the code), and **Translation settings**. Reading,
porting and building run on your computer; only translating with Claude sends anything
out (each function's code, to api.anthropic.com).

**Translation settings** (in the ⋯ menu, or **Change** on the summary) choose who translates: Claude
Haiku 5.5 (the default, about $0.20 for a mod the size of Create), Sonnet 5.5, Opus 5.5,
or a model on your own computer through [Ollama](https://ollama.com), which is free
and sends nothing anywhere. Your Claude API key goes in the system's credential store
(Windows Credential Manager, the macOS Keychain, or the Secret Service on Linux), never
in a file; the app's page never sees it again once saved, and it is sent only to
api.anthropic.com. `ANTHROPIC_API_KEY` works too.

Building needs the [Horizon launcher](https://playhorizon.dev) installed and signed
in. Native mods are not sandboxed, so Horizon Porter asks you to confirm you trust the
code before every build, as the launcher does.

Get the installer for your system from the
[latest release](https://github.com/kevinbonomi2-bit/horizon-porter/releases/latest):
`Horizon.Porter_<version>_x64-setup.exe` (Windows), `Horizon.Porter_<version>_<chip>.app.zip`
(macOS: `aarch64` for Apple Silicon, `x86_64` for Intel) or
`Horizon.Porter_<version>_amd64.deb` (Linux, needs WebKitGTK 4.1). The builds are not
signed yet: Windows may show a SmartScreen warning (More info, then Run anyway), and
on macOS run `xattr -dr com.apple.quarantine "Horizon Porter.app"` once after unzipping.

## What it does

Horizon can't run Java mod jars, and its launcher already carries over a mod's data
and assets. horizon-porter is for the rest, **the code**:

1. **Scan** your mod's source: every class, Mixin, registry entry and asset, and what
   each one depends on.
2. **Report** what ports automatically, what needs a human, and what is blocked, such as
   a Horizon hook point that doesn't exist yet.
3. **Port** it: a Horizon content mod for blocks, items, data and assets, and a native
   mod bundle for the code, with block entities as saved state and hooks wired. Mixins are translated to horizon-mixins, and API calls are mapped through
   Horizon's port ledger and the modkit runtime. AI translates the method bodies that
   rules can't, and the Rust compiler checks every result.
4. **Check** it against Horizon, and keep track of what is left, class by class.

The goal is to make large mods (think Create) a realistic porting project, rather than
a rewrite from scratch. It won't be one click, and the reports always say what is left.

## The command line

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

## Usage

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
```

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

Each **block entity** becomes a state struct in `runtime.rs`, saved with its block
under the keys `saveAdditional` writes, plus a `tick_<name>` stub for its Java tick.
Horizon's `native_block_entity` hooks give the blocks their block entity and run the
tick on the server, saving what changed. Port the tick's body and the block works:
`/data get block <x y z>` shows its saved values. `PORTING.md` lists every block's
template and why, and anything left out.

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

**What it costs.** `--dry-run` prints an estimate before anything is spent, and more
than five functions need `--yes`. Claude Haiku 5.5 is the default: about $0.01 for a
small mod like Farmer's Delight and about $0.20 for Create's 95 functions.
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
or a built `.jar`. The source gives the full picture. A jar gives classes, metadata and
resources, but not Mixin details or registrations.

`scan` tells you what the mod is made of: classes, Mixins and their verdicts, registry
entries, resources, and what it depends on (Minecraft, the loader API, other mods).
`report` adds the details: each Mixin that needs work and why, the Minecraft and loader
classes the mod uses most, the Minecraft classes it extends, the libraries it needs
first, and any direct OpenGL use. [docs/CREATE.md](docs/CREATE.md#what-the-scanner-found)
shows what it found in Create.

## Building

Needs Rust 1.90 or later ([rustup](https://rustup.rs/)). On Windows, also the Visual
Studio C++ Build Tools.

```
cargo build --release
cargo test
./target/release/horizon-porter --help
```

The desktop app (`app/`, built with [Tauri](https://tauri.app)) builds with
`cargo build -p horizon-porter-app`. On Linux it needs WebKitGTK:
`sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`.
Installers come from `npx @tauri-apps/cli@2 build` in `app/`.

## Layout

```
tools/horizon_hooks.py   regenerates the hook table from a Horizon checkout
crates/porter-cli        the horizon-porter command
crates/porter-report     reports: terminal text and the HTML page
crates/porter-model      shared types
crates/porter-java       reads Java mods
crates/porter-map        Horizon's hook table, Mixin and event mappings, the Horizon port lookup
crates/porter-mixin      Mixin translation
crates/porter-content    blocks, items, data and assets → a content mod (.hmod)
crates/porter-translate  Java → Rust: the native mod, block entities, PORTING.md
crates/porter-verify     builds ported mods with the Horizon launcher
app/                     Horizon Porter, the desktop app (Tauri; the page is in app/ui)
modkit/                  the runtime ported mods build on
docs/                    architecture, roadmap, the Create case study
```

## Design

The desktop app and the HTML report were designed with two open design skills:
[unslop](https://github.com/heydamir/unslop) by Damir Sokolovsky
([@heydamir](https://x.com/heydamir)), CC BY 4.0, whose tokens (the graphite ramp,
spacing, type and motion values) the stylesheets adapt; and
[oil-ui](https://github.com/oil-oil/oil-ui) (MIT), whose process (one subject and one
action per screen, structure before style, review from real screenshots) shaped the
screens.

## Licence

MIT. Port only mods whose licence allows it. horizon-porter is planned to read each
mod's licence and warn you when it doesn't.

`crates/porter-map/data/horizon-hooks.json` is derived from
[Horizon](https://github.com/horizon-rust/horizon)'s source (MIT, Copyright (c) 2026
Ludovic Marie and the Horizon contributors).
