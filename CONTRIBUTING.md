# Contributing to horizon-porter

Thanks for helping. horizon-porter is useful only if what it says about a mod is true,
so most contributions are about accuracy: a verdict that is wrong, a registration it
misses, a Java API it does not map yet.

## Ground rules

- **Honest output.** Every verdict, count and generated file must be explainable. When
  the tool cannot tell, it says so (a warning, a "partial" verdict, a TODO) rather than
  guessing quietly.
- **Never destroy work.** Nothing writes over a folder with files in it unless the user
  asks, and then only the tool's own files.
- **Mods belong to their authors.** Do not commit other people's mod code or assets,
  not even as test fixtures. Write a small fixture that reproduces the pattern instead
  (see `crates/porter-java/tests/fixtures/`).
- **Horizon moves fast.** Hook signatures and revisions come from Horizon's own files
  (`native-hooks.json`, `native_hooks.rs`, horizon-mods' native mods). Say which commit
  you checked against.

## Building and testing

You need Rust 1.90 or later. On Windows, the Visual Studio C++ Build Tools; on Linux,
WebKitGTK for the desktop app (see the README).

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same on Linux, Windows and macOS. The desktop app builds a real installer
in CI too, so check `cargo clippy -p horizon-porter-app` when you change `app/`.

To try a change on a real mod, scan its public source locally:

```
cargo run --release -p porter-cli -- report path/to/mod --html report.html
```

## Where things live

| You want to... | Look at |
| --- | --- |
| Fix how Java source is read (classes, annotations, calls) | `crates/porter-java/src/source.rs` |
| Recognise a new registration pattern | `registrations` in `crates/porter-java/src/lib.rs`, with a case in the neoforge-demo fixture |
| Change a Mixin verdict | `crates/porter-java/src/mixin.rs`, following horizon-mixins' feature map |
| Map a Fabric or NeoForge API to a Horizon hook | `CLIENT_HOOKS` in `crates/porter-map/src/lib.rs` |
| Wire a new hook in generated bundles | `crates/porter-translate/src/bundle.rs`, plus its signature in `tests/horizon_mock.rs` |
| Change the reports | `crates/porter-report/` (the HTML page is self-contained: no scripts, nothing external) |
| Change the desktop app | `app/src/main.rs` (commands) and `app/ui/` (the page) |

## Design

The app and the HTML report follow a written direction: graphite surfaces, one ember
accent for the main action, hairline borders, a 4px grid, sentence case, no glows or
gradients, and the inventory slot as the one signature detail. It came from the
[unslop](https://github.com/heydamir/unslop) and [oil-ui](https://github.com/oil-oil/oil-ui)
skills. Keep new UI inside it, and check changes from real screenshots in light and
dark before sending them.

## Pull requests

- One change per pull request, with a test that fails without it.
- Explain *why* in the description: the mod or pattern that showed the problem.
- Keep the docs true: the README, `docs/ROADMAP.md` and `PORTING.md` output describe
  what the tool really does.

By contributing, you agree that your work is released under the MIT licence.
