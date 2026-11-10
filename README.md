# horizon-porter

Port Minecraft Java Edition mods (Fabric, NeoForge, Forge; Minecraft 26.3) to
[Horizon](https://github.com/horizon-rust/horizon), the Rust rewrite of Minecraft.

Drop a mod's `.jar` on the app, press one button, and play it in Horizon: its blocks, items,
recipes and textures, and the Java code behind them, translated to Rust and checked by the
compiler and in the game.

> **Status: early, and moving fast.** Content mods and client mods port well, and mods with
> machines and block entities are getting there: Farmer's Delight's cooking pot and cutting board
> play as they do in Java. Every port says what came over and what didn't.
> Not affiliated with Horizon, Mojang or Microsoft.

## Get it

Download **Horizon Porter** from the
[latest release](https://github.com/kevinbonomi2-bit/horizon-porter/releases/latest):

| System | File |
| --- | --- |
| Windows | `Horizon.Porter_<version>_x64-setup.exe` |
| macOS | `Horizon.Porter_<version>_<chip>.app.zip` (`aarch64` Apple Silicon, `x86_64` Intel) |
| Linux | `Horizon.Porter_<version>_amd64.deb` (needs WebKitGTK 4.1) |

The builds aren't signed yet: on Windows choose *More info → Run anyway*; on macOS run
`xattr -dr com.apple.quarantine "Horizon Porter.app"` once. You also need the
[Horizon launcher](https://playhorizon.dev), installed and signed in.

## Use it

1. Drop a mod's `.jar` (or its source folder) on the window. You get a report: what ports,
   what needs work, and what Horizon can't do yet.
2. Press **Make it work in Horizon**. It shows what carries over, what translating the code
   costs, and which of your Horizon instances it goes in.
3. Say yes. It ports the mod, translates its code, adds it to that instance, builds it, then
   tries it in a test world: it places every block and right-clicks the ones that act.
4. Press **Play**.

A mod that is only content (blocks, items, recipes) needs no translation or build.

**Translation** uses Claude Haiku 5.5 by default (about $2 for Farmer's Delight; the app shows
the price before anything is spent), or a model on your own computer through
[Ollama](https://ollama.com), free and offline. Your API key stays in the system's credential
store. Reading, porting and building all run on your computer.

**Native mods aren't sandboxed**, so the app asks you to confirm you trust the code before every
build, as the Horizon launcher does. Port only mods whose licence allows it.

## What ports today

| Kind of mod | Today |
| --- | --- |
| Blocks, items, recipes, loot, tags, models, textures | Yes |
| Foods, drinks, tools, weapons, armor | Yes |
| Blocks that act: clicks, placement, crops, ticks, comparators | Yes, translated from the Java |
| Machines: menus, recipes, slots, hoppers, renderers | Yes, for most (Farmer's Delight's cooking pot) |
| Client mods: keys, HUD, commands, settings pages | Yes |
| Mod mobs | Partly: they spawn, but look and act like the vanilla mob they copy |
| New biomes and world generation | Not yet (Horizon has no hook for it) |
| Shaders and rendering engines | Not yet |

[docs/CENSUS.md](docs/CENSUS.md) measures this over 150 popular mods.

## Command line

The same porter, for mod developers and scripts. Get the `horizon-porter` archive for your
system from the [latest release](https://github.com/kevinbonomi2-bit/horizon-porter/releases/latest).

```
horizon-porter report my-mod.jar --html report.html   # what ports, as a page
horizon-porter port my-mod.jar --out my-mod-horizon   # write the Horizon mod
horizon-porter translate my-mod-horizon --dry-run     # what translating costs
horizon-porter translate my-mod-horizon               # translate the code (Claude Haiku 5.5)
horizon-porter install my-mod-horizon --build --trust-native-code
horizon-porter check my-mod-horizon                   # try it in a test world
```

Add `--api` to `port` to also write the mod for Horizon's new mod API (experimental, on the
[arismoko/horizon](https://github.com/arismoko/horizon) fork for now).
[docs/USAGE.md](docs/USAGE.md) explains every command.

## Build from source

Needs Rust 1.90 or later; on Windows, the Visual Studio C++ Build Tools.

```
cargo build --release
cargo test
```

The desktop app (`app/`, [Tauri](https://tauri.app)) builds with
`cargo build -p horizon-porter-app`; on Linux it needs
`libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev pkg-config`.

## More

- [docs/USAGE.md](docs/USAGE.md): every command and what a port contains
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/ROADMAP.md](docs/ROADMAP.md): how it works and what's next
- [docs/API-TRIAL.md](docs/API-TRIAL.md): mods on Horizon's new mod API
- [docs/releases/](docs/releases/): release notes
- [CONTRIBUTING.md](CONTRIBUTING.md): issues, mapping requests and pull requests

The app and the HTML report were designed with [unslop](https://github.com/heydamir/unslop)
by Damir Sokolovsky (CC BY 4.0), whose tokens the stylesheets adapt, and
[oil-ui](https://github.com/oil-oil/oil-ui) (MIT).

## Licence

MIT. `crates/porter-map/data/horizon-hooks.json` is derived from
[Horizon](https://github.com/horizon-rust/horizon)'s source (MIT, Copyright (c) 2026 Ludovic
Marie and the Horizon contributors).
