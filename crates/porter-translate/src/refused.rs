//! When the game's build refuses a port's translated code. `translate` checks the
//! runtime with `cargo test`, but a hook helper only compiles inside Horizon, so a
//! helper that calls something the game doesn't have (a `Game` helper on a
//! `GameLevel`) fails the build of the whole instance. Rather than leave the player
//! with nothing to play, each function the build's errors fall in is put back to
//! its stub (a TODO `translate` ports again), and the build is run once more: the
//! mod plays without those functions.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::ai::units;

/// A translated function the build refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// Its file in the port: `mixins/demo/smoke_source.rs`.
    pub file: String,
    pub function: String,
    /// The build's first error in it: `no method named native_data found ...`.
    pub error: String,
}

/// The translated functions of the port in `port` that the errors of `log` (the
/// launcher's build.log) fall in, `tree` being the Horizon source the build compiled
/// (each error's `--> path:line` is relative to it).
pub fn find(port: &Path, log: &str, tree: &Path) -> Result<Vec<Refused>> {
    let mixins = mixins_dir(port)?;
    let rel = |p: &Path| {
        p.strip_prefix(port)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    };
    // Every translated function: its name, file, and whether it's in runtime.rs
    // (inlined in a module) or a helper (pasted into a hook, its name unique).
    let mut helpers: BTreeMap<String, String> = BTreeMap::new();
    let mut runtime: BTreeMap<String, String> = BTreeMap::new();
    // Every helper `port` wrote, translated or not: an error in one it wrote
    // whole (a block's click) is no translated function's.
    let mut all_helpers: std::collections::BTreeSet<String> = Default::default();
    for entry in
        std::fs::read_dir(&mixins).with_context(|| format!("reading {}", mixins.display()))?
    {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path)?;
        let is_runtime = path.file_name().is_some_and(|n| n == "runtime.rs");
        if !is_runtime {
            all_helpers.extend(units::function_names(&source));
        }
        for f in units::ported_functions(&source) {
            if is_runtime {
                runtime.insert(f, rel(&path));
            } else {
                helpers.insert(f, rel(&path));
            }
        }
    }
    let module = module_name(&mixins);
    let mut out: Vec<Refused> = Vec::new();
    for (error, file, line) in errors(log) {
        let Ok(text) = std::fs::read_to_string(tree.join(&file)) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        let Some(found) = enclosing(
            &lines,
            line,
            &helpers,
            &all_helpers,
            &runtime,
            module.as_deref(),
        ) else {
            continue;
        };
        if !out
            .iter()
            .any(|r| r.file == found.0 && r.function == found.1)
        {
            out.push(Refused {
                file: found.0,
                function: found.1,
                error,
            });
        }
    }
    Ok(out)
}

/// Puts each refused function back to its stub; what it put back, as lines a
/// player reads.
pub fn revert(port: &Path, refused: &[Refused]) -> Result<Vec<String>> {
    let mut done = Vec::new();
    for r in refused {
        let path = port.join(&r.file);
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        if let Some(stub) = units::unport(&source, &r.function, &r.error) {
            std::fs::write(&path, stub).with_context(|| format!("writing {}", path.display()))?;
            done.push(format!("{} in {}: {}", r.function, r.file, r.error));
        }
    }
    Ok(done)
}

/// [`find`] then [`revert`]: what it put back, empty when the errors are in nothing
/// it can put back. The port is to be packaged and installed again after.
pub fn repair(port: &Path, log: &str, tree: &Path) -> Result<Vec<String>> {
    let refused = find(port, log, tree)?;
    revert(port, &refused)
}

/// `mixins/<id>/` of a port.
fn mixins_dir(port: &Path) -> Result<PathBuf> {
    let mixins = port.join("mixins");
    std::fs::read_dir(&mixins)
        .with_context(|| format!("reading {}", mixins.display()))?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.join("horizon-mixin.toml").is_file())
        .with_context(|| format!("no mod under {}", mixins.display()))
}

/// The module runtime.rs becomes: `ported_demo` (horizon-mixin.toml's `[[module]]`).
fn module_name(mixins: &Path) -> Option<String> {
    let text = std::fs::read_to_string(mixins.join("horizon-mixin.toml")).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    table
        .get("module")?
        .as_array()?
        .iter()
        .find_map(|m| m.get("name")?.as_str().map(str::to_string))
}

/// The errors of a cargo log: (message, file, 1-based line), warnings left out.
fn errors(log: &str) -> Vec<(String, String, usize)> {
    let mut out = Vec::new();
    let mut message: Option<String> = None;
    for line in log.lines() {
        let t = line.trim();
        if t.starts_with("error[")
            || (t.starts_with("error:") && !t.starts_with("error: could not compile"))
        {
            message = Some(
                t.split_once(": ")
                    .map_or(t, |(_, m)| m)
                    .replace('`', "")
                    .chars()
                    .take(160)
                    .collect(),
            );
            continue;
        }
        if t.starts_with("warning") {
            message = None;
            continue;
        }
        if let (Some(m), Some(at)) = (&message, t.strip_prefix("--> ")) {
            // `crates\horizon-client\src\game\native_hooks.rs:2161:26`
            let mut parts = at.rsplitn(3, ':');
            let (_col, line, file) = (parts.next(), parts.next(), parts.next());
            if let (Some(file), Some(line)) = (file, line.and_then(|l| l.parse().ok())) {
                out.push((m.clone(), file.replace('\\', "/"), line));
            }
            message = None;
        }
    }
    out
}

/// The translated function line `line` (1-based) of `lines` falls in: the nearest
/// `fn` above it that is one of `helpers`, or one of `runtime` at the top level of
/// `module`'s body. (file, function).
fn enclosing(
    lines: &[&str],
    line: usize,
    helpers: &BTreeMap<String, String>,
    all_helpers: &std::collections::BTreeSet<String>,
    runtime: &BTreeMap<String, String>,
    module: Option<&str>,
) -> Option<(String, String)> {
    let at = line.checked_sub(1)?.min(lines.len().checked_sub(1)?);
    // runtime.rs's module: where it opens and the indent of its own items.
    let opens = module.and_then(|m| {
        let head = format!("mod {m} ");
        let head_brace = format!("mod {m}{{");
        lines[..=at]
            .iter()
            .rposition(|l| l.contains(&head) || l.contains(&head_brace))
    });
    for i in (0..=at).rev() {
        let l = lines[i];
        let Some(name) = fn_name(l) else {
            continue;
        };
        if let Some(file) = helpers.get(&name) {
            return Some((file.clone(), name));
        }
        // A helper `port` wrote whole: the error is its, not a translated one's.
        if all_helpers.contains(&name) {
            return None;
        }
        if let (Some(file), Some(open)) = (runtime.get(&name), opens) {
            let indent = |s: &str| s.len() - s.trim_start().len();
            if i > open && indent(l) == indent(lines[open]) + 4 {
                return Some((file.clone(), name));
            }
        }
    }
    None
}

/// `    pub(crate) fn tick_crank<'a>(` → `tick_crank`.
fn fn_name(line: &str) -> Option<String> {
    let t = line.trim_start();
    let t = t
        .strip_prefix("pub(crate) ")
        .or_else(|| t.strip_prefix("pub(super) "))
        .or_else(|| t.strip_prefix("pub "))
        .unwrap_or(t);
    let rest = t.strip_prefix("fn ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = r#"warning: unused variable: `machine`
     --> crates\horizon-client\src\game.rs:12:61
error[E0599]: no method named `native_data` found for reference `&game::GameLevel<'_>` in the current scope
    --> crates\horizon-client\src\game\native_hooks.rs:7:26
     |
error[E0308]: mismatched types
    --> crates\horizon-client\src\game.rs:9:9
error: could not compile `horizon-client` (lib) due to 2 previous errors
"#;

    #[test]
    fn errors_name_their_place() {
        let e = errors(LOG);
        assert_eq!(e.len(), 2, "{e:?}");
        assert_eq!(e[0].1, "crates/horizon-client/src/game/native_hooks.rs");
        assert_eq!(e[0].2, 7);
        assert!(e[0].0.starts_with("no method named native_data"));
    }

    #[test]
    fn a_refused_port_goes_back_to_its_stub() {
        let dir = std::env::temp_dir().join(format!("porter-refused-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mixins = dir.join("port/mixins/demo");
        std::fs::create_dir_all(&mixins).unwrap();
        std::fs::write(
            mixins.join("horizon-mixin.toml"),
            "[[module]]\nid = \"runtime\"\nname = \"ported_demo\"\n",
        )
        .unwrap();
        std::fs::write(
            mixins.join("smoke.rs"),
            "// Horizon's `native_smoke_source`.\n//\n// Ported by horizon-porter translate (claude-haiku-5-5) from these; review it:\n// - `@Inject` isSmoke on `isSmokeSource`\nfn demo_smoke_source(game: &Game, smoke: &mut Option<bool>) {\n    let x = game.native_data(\"a\");\n}\n",
        )
        .unwrap();
        std::fs::write(
            mixins.join("runtime.rs"),
            "/// Ported by horizon-porter translate (claude-haiku-5-5) from a.Crank. Review it.\npub fn tick(n: i32) -> i32 {\n    n + \"x\"\n}\n\nimpl A {\n    fn tick(&self) {}\n}\n",
        )
        .unwrap();
        // The tree the game built: the helper pasted in a hook, runtime.rs inlined.
        let tree = dir.join("tree/crates/horizon-client/src/game");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(
            tree.join("native_hooks.rs"),
            "impl Game {\n    fn native_smoke_source(&self) {\n        fn demo_smoke_source(game: &Game) {\n            fn in_tag() {\n                let x = 1;\n            }\n            let y = game.native_data(\"a\");\n        }\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("tree/crates/horizon-client/src/game.rs"),
            "mod ported_demo {\n    pub fn tick(n: i32) -> i32 {\n        n + \"x\"\n    }\n}\n// a line\n// a line\n// a line\n",
        )
        .unwrap();
        let port = dir.join("port");
        let refused = find(&port, LOG, &dir.join("tree")).unwrap();
        assert_eq!(
            refused
                .iter()
                .map(|r| (r.file.as_str(), r.function.as_str()))
                .collect::<Vec<_>>(),
            [
                ("mixins/demo/smoke.rs", "demo_smoke_source"),
                ("mixins/demo/runtime.rs", "tick")
            ]
        );
        let done = revert(&port, &refused).unwrap();
        assert_eq!(done.len(), 2);
        let helper = std::fs::read_to_string(mixins.join("smoke.rs")).unwrap();
        assert!(
            helper.contains("// TODO: port into this function:\n// Its port was put back"),
            "{helper}"
        );
        assert!(helper.contains("- `@Inject` isSmoke on `isSmokeSource`"));
        assert!(helper.contains("fn demo_smoke_source(game: &Game, smoke: &mut Option<bool>) {\n    let _ = (game, smoke);\n    Default::default()\n}"), "{helper}");
        let runtime = std::fs::read_to_string(mixins.join("runtime.rs")).unwrap();
        assert!(
            runtime.starts_with("/// TODO: port from a.Crank.\n"),
            "{runtime}"
        );
        // translate finds both again.
        assert_eq!(units::find("runtime.rs", &runtime).len(), 1);
        assert_eq!(units::find("smoke.rs", &helper).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
