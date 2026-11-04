# horizon-porter

A tool for mod developers to port Minecraft Java Edition mods (Fabric, NeoForge,
Forge; Minecraft 26.3) to [Horizon](https://github.com/horizon-rust/horizon), the
Rust rewrite of Minecraft, as native Horizon mods.

> **Status: early.** The scanner works (`scan`, `report`); nothing ports yet. The plan
> is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/ROADMAP.md](docs/ROADMAP.md).
> Not affiliated with Horizon, Mojang or Microsoft.

## What it does

Horizon can't run Java mod jars, and its launcher already carries over a mod's data
and assets. horizon-porter is for the rest, **the code**:

1. **Scan** your mod's source: every class, Mixin, registry entry and asset, and what
   each one depends on.
2. **Report** what ports automatically, what needs a human, and what is blocked, such as
   a Horizon hook point that doesn't exist yet.
3. **Port** it: a Horizon content mod for data and assets, and a native mod bundle for the
   code. Mixins are translated to horizon-mixins, and API calls are mapped through
   Horizon's port ledger and the modkit runtime. AI translates the method bodies that
   rules can't, and the Rust compiler checks every result.
4. **Check** it against Horizon, and keep track of what is left, class by class.

The goal is to make large mods (think Create) a realistic porting project, rather than
a rewrite from scratch. It won't be one click, and the reports always say what is left.

## Install

Download the archive for your system from the
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

### Porting a client mod

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

`cargo test` works in the generated folder straight away. To build the game with it,
use Horizon's tools (the steps are at the end of `PORTING.md`). `port` never
overwrites a folder that has files unless you pass `--force`, and then only replaces
its own files.

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

## Layout

```
crates/porter-cli        the horizon-porter command
crates/porter-report     reports: terminal text and the HTML page
crates/porter-model      shared types
crates/porter-java       reads Java mods
crates/porter-map        Java → Horizon symbol and Mixin mappings
crates/porter-mixin      Mixin translation
crates/porter-content    data and assets → .hmod
crates/porter-translate  Java → Rust code translation
crates/porter-verify     builds and checks against Horizon
modkit/                  the runtime ported mods build on
docs/                    architecture, roadmap, the Create case study
```

## Licence

MIT. Port only mods whose licence allows it. horizon-porter is planned to read each
mod's licence and warn you when it doesn't.
