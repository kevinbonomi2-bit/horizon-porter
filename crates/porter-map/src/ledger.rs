//! Where Horizon ports a vanilla method: the Rust functions whose comments name it,
//! read from a Horizon source checkout (the launcher keeps one for each Horizon it
//! built native mods on). A lighter cousin of Horizon's own `tools/port_ledger.py`,
//! which needs the vanilla jar: this reads only Horizon's comments.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One Horizon function that names a vanilla method.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Port {
    /// How it names the method: lower is stronger.
    pub rank: Rank,
    /// The file, relative to the checkout, with `/` separators.
    pub file: String,
    /// The line of the function.
    pub line: usize,
    pub function: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    /// Its doc opens with the method, Horizon's house style for a port.
    Ported,
    /// Its doc names the method.
    Documented,
    /// A comment in its body names it.
    Mentioned,
}

impl Rank {
    pub fn describe(self) -> &'static str {
        match self {
            Rank::Ported => "ports it",
            Rank::Documented => "its doc names it",
            Rank::Mentioned => "a comment in it names it",
        }
    }
}

/// The Horizon source checkout to read: `HORIZON_SOURCE`, else the newest one the
/// launcher keeps (`<launcher data>/native/source/<commit>`).
pub fn horizon_source() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("HORIZON_SOURCE").map(PathBuf::from)
        && dir.join("crates").is_dir()
    {
        return Some(dir);
    }
    let mut data_dirs: Vec<PathBuf> = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        data_dirs.push(PathBuf::from(appdata));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        data_dirs.push(PathBuf::from(xdg));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        data_dirs.push(home.join(".local/share"));
        data_dirs.push(home.join("Library/Application Support"));
    }
    data_dirs
        .iter()
        .filter_map(|d| {
            std::fs::read_dir(d.join("horizon-launcher").join("native").join("source")).ok()
        })
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("crates").is_dir())
        .max_by_key(|p| {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH)
        })
}

/// The Horizon functions that name each of `methods` (`FarmBlock.turnToDirt`),
/// strongest first, at most `limit` each. Methods no comment names are left out.
pub fn ports(source: &Path, methods: &[String], limit: usize) -> BTreeMap<String, Vec<Port>> {
    let mut out: BTreeMap<String, Vec<Port>> = BTreeMap::new();
    if methods.is_empty() {
        return out;
    }
    let mut stack = vec![source.join("crates")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path
                    .file_name()
                    .is_some_and(|n| n != "target" && n != "tests")
                {
                    stack.push(path);
                }
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if !methods.iter().any(|m| text.contains(m.as_str())) {
                continue;
            }
            let rel = path
                .strip_prefix(source)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            scan_file(&text, &rel, methods, &mut out);
        }
    }
    for ports in out.values_mut() {
        ports.sort();
        ports.dedup_by(|a, b| a.file == b.file && a.line == b.line);
        ports.truncate(limit);
    }
    out
}

fn scan_file(text: &str, rel: &str, methods: &[String], out: &mut BTreeMap<String, Vec<Port>>) {
    let lines: Vec<&str> = text.lines().collect();
    // Tests are no ports: stop at the file's test module.
    let end = lines
        .iter()
        .position(|l| l.trim() == "#[cfg(test)]")
        .unwrap_or(lines.len());
    for (i, line) in lines[..end].iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(comment) = trimmed.strip_prefix("//") else {
            continue;
        };
        let is_doc = comment.starts_with('/') && !comment.starts_with("//");
        let body = comment.trim_start_matches(['/', '!']).trim_start();
        for method in methods {
            if !names(body, method) {
                continue;
            }
            let (rank, found) = if is_doc {
                // The doc's first line, and the function it documents.
                let first = lines[..i]
                    .iter()
                    .rev()
                    .take_while(|l| l.trim_start().starts_with("///"))
                    .count()
                    == 0;
                let opens = first && body.trim_start_matches('`').starts_with(method.as_str());
                let rank = if opens {
                    Rank::Ported
                } else {
                    Rank::Documented
                };
                (rank, next_fn(&lines, i))
            } else {
                (Rank::Mentioned, enclosing_fn(&lines, i))
            };
            if let Some((line, function)) = found
                && !is_test(&lines, line)
            {
                out.entry(method.clone()).or_default().push(Port {
                    rank,
                    file: rel.to_string(),
                    line: line + 1,
                    function,
                });
            }
        }
    }
}

/// Whether `text` names `method` as a whole (`Gui.render`, not `Gui.renderHotbar`
/// or `NewGui.render`).
fn names(text: &str, method: &str) -> bool {
    text.match_indices(method).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + method.len()..].chars().next();
        let part = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        !part(before) && !part(after)
    })
}

/// The function a doc comment at `i` documents: the next `fn` within a few lines.
fn next_fn(lines: &[&str], i: usize) -> Option<(usize, String)> {
    lines
        .iter()
        .enumerate()
        .skip(i + 1)
        .take(40)
        .find_map(|(j, l)| fn_name(l).map(|n| (j, n)))
}

/// The function a comment at `i` is inside: the nearest `fn` above it that is
/// indented less.
fn enclosing_fn(lines: &[&str], i: usize) -> Option<(usize, String)> {
    let indent = |l: &str| l.len() - l.trim_start().len();
    let own = indent(lines[i]);
    lines[..i].iter().enumerate().rev().find_map(|(j, l)| {
        (indent(l) < own)
            .then(|| fn_name(l).map(|n| (j, n)))
            .flatten()
    })
}

/// Whether the function at `line` is a test (`#[test]` above it).
fn is_test(lines: &[&str], line: usize) -> bool {
    lines[..line]
        .iter()
        .rev()
        .take_while(|l| {
            let t = l.trim_start();
            t.starts_with("#[") || t.starts_with("///")
        })
        .any(|l| l.trim_start().starts_with("#[test"))
}

/// `pub(crate) fn tick_rain(` → `tick_rain`.
fn fn_name(line: &str) -> Option<String> {
    let t = line.trim_start();
    if t.starts_with("//") {
        return None;
    }
    let at = t.find("fn ")?;
    let before = &t[..at];
    if !before.is_empty()
        && !before.split_whitespace().all(|w| {
            w.starts_with("pub") || ["const", "async", "unsafe", "extern", "\"C\""].contains(&w)
        })
    {
        return None;
    }
    let name: String = t[at + 3..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "
/// `LivingEntity.jumpFromGround`: the jump.
pub fn jump(entity: &mut Living) {
    let y = 0.42;
}

impl Mob {
    /// Moves it, as `Entity.move` does.
    pub(crate) fn step(&mut self) {
        // Rabbit.jumpFromGround: a hop.
        self.hop();
    }
}

#[cfg(test)]
mod tests {
    /// `Entity.move` stops at walls.
    #[test]
    fn stops_at_walls() {}
}
";

    #[test]
    fn finds_the_functions_that_name_a_method() {
        let mut out = BTreeMap::new();
        let methods = [
            "LivingEntity.jumpFromGround".to_string(),
            "Entity.move".to_string(),
            "Rabbit.jumpFromGround".to_string(),
            "FarmBlock.turnToDirt".to_string(),
        ];
        scan_file(SRC, "crates/x/src/a.rs", &methods, &mut out);
        let jump = &out["LivingEntity.jumpFromGround"][0];
        assert_eq!(
            (jump.rank, jump.line, jump.function.as_str()),
            (Rank::Ported, 3, "jump")
        );
        assert_eq!(out["Entity.move"].len(), 1, "the test is no port");
        let step = &out["Entity.move"][0];
        assert_eq!(
            (step.rank, step.function.as_str()),
            (Rank::Documented, "step")
        );
        let hop = &out["Rabbit.jumpFromGround"][0];
        assert_eq!((hop.rank, hop.function.as_str()), (Rank::Mentioned, "step"));
        assert!(!out.contains_key("FarmBlock.turnToDirt"));
        assert!(!names("Gui.renderHotbar", "Gui.render"));
        assert!(!names("NewGui.render", "Gui.render"));
        assert!(names("(`Gui.render`)", "Gui.render"));
    }
}
