// Terminal text: the summary `scan` prints and the full breakdown `report` prints.
// The helpers here are shared with the HTML page.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use porter_model::{InputKind, ModScan, Side, SymbolOrigin};

/// The one-screen summary printed by `scan`.
pub fn summary(scan: &ModScan) -> String {
    let mut out = String::new();
    header(scan, &mut out);

    let lines: usize = scan.classes.iter().map(|c| c.lines).sum();
    let methods: usize = scan.classes.iter().map(|c| c.methods).sum();
    let client = scan
        .classes
        .iter()
        .filter(|c| c.side == Side::Client)
        .count();
    let mut code = format!("{} classes", n(scan.classes.len()));
    if lines > 0 {
        let _ = write!(code, ", {} lines", n(lines));
    }
    let _ = write!(code, ", {} methods ({} client-only)", n(methods), n(client));
    row(&mut out, "Code", &code);

    let members: Vec<_> = scan.mixins.iter().flat_map(|m| &m.members).collect();
    let mut mixins = format!(
        "{} classes in {} configs",
        n(scan.mixins.len()),
        n(scan.mixin_configs.len())
    );
    if !members.is_empty() {
        let _ = write!(mixins, ", {} members", n(members.len()));
    }
    row(&mut out, "Mixins", &mixins);
    if !members.is_empty() {
        let text = verdict_tally(scan)
            .into_iter()
            .filter(|(_, c)| *c > 0)
            .map(|(k, c)| format!("{} {k}", n(c)))
            .collect::<Vec<_>>()
            .join(" · ");
        row(&mut out, "", &text);
        let outlook = porter_map::reach::outlook(scan);
        if outlook.total() > 0 {
            row(&mut out, "Portable?", &outlook.summary);
        }
    }

    if scan.input_kind == InputKind::SourceTree {
        let by_registry = registry_counts(scan);
        let mut text = format!("{} entries", n(scan.registrations.len()));
        if !by_registry.is_empty() {
            let _ = write!(
                text,
                ": {}",
                by_registry
                    .iter()
                    .take(6)
                    .map(|(r, c)| format!("{r} {}", n(*c)))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if by_registry.len() > 6 {
                text.push_str(", ...");
            }
        }
        row(&mut out, "Registries", &text);
    }

    let r = &scan.resources;
    row(
        &mut out,
        "Resources",
        &format!(
            "{} asset files, {} data files (namespaces: {})",
            n(r.assets()),
            n(r.data()),
            if r.namespaces.is_empty() {
                "none".to_string()
            } else {
                r.namespaces.join(", ")
            }
        ),
    );

    let counts = porter_java::reference_counts(scan);
    let depends = [
        SymbolOrigin::Minecraft,
        SymbolOrigin::LoaderApi,
        SymbolOrigin::Library,
        SymbolOrigin::Platform,
    ]
    .iter()
    .filter_map(|o| {
        counts
            .get(o)
            .map(|m| format!("{} {} classes", o.label(), n(m.len())))
    })
    .collect::<Vec<_>>()
    .join(" · ");
    row(&mut out, "Depends on", &depends);

    let libraries = library_groups(scan);
    if !libraries.is_empty() {
        let text = libraries
            .iter()
            .take(5)
            .map(|(g, c)| format!("{g} ({})", n(*c)))
            .collect::<Vec<_>>()
            .join(", ");
        row(&mut out, "Libraries", &text);
    }
    for (name, note, classes) in known_libraries(scan) {
        row(
            &mut out,
            "",
            &format!("{name} ({} classes): {note}", n(classes)),
        );
    }
    if !scan.bundled_jars.is_empty() {
        row(
            &mut out,
            "Bundled",
            &format!("{} jars", n(scan.bundled_jars.len())),
        );
    }
    if !scan.warnings.is_empty() {
        row(
            &mut out,
            "Warnings",
            &format!(
                "{} (horizon-porter report lists them)",
                n(scan.warnings.len())
            ),
        );
    }
    out
}

/// The full report printed by `report`.
pub fn full(scan: &ModScan, all: bool, top: usize) -> String {
    let mut out = summary(scan);

    // Mixins.
    if !scan.mixins.is_empty() {
        section(&mut out, "Mixins");
        let mut by_annotation: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
        for m in scan.mixins.iter().flat_map(|m| &m.members) {
            *by_annotation
                .entry(m.annotation.as_str())
                .or_default()
                .entry(m.verdict.name())
                .or_default() += 1;
        }
        for (annotation, verdicts) in &by_annotation {
            let total: usize = verdicts.values().sum();
            let detail = verdicts
                .iter()
                .map(|(v, c)| format!("{} {v}", n(*c)))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "  @{annotation:<24} {:>5}   {detail}", n(total));
        }

        let other_mod_targets = other_mod_targets(scan);
        if !other_mod_targets.is_empty() {
            let _ = writeln!(
                out,
                "\n  Mixins into other mods or the loader (port or replace those first):"
            );
            for t in other_mod_targets.iter().take(top) {
                let _ = writeln!(out, "    {t}");
            }
        }

        let _ = writeln!(
            out,
            "\n  {}:",
            if all {
                "Every member"
            } else {
                "Members that need work (partial or blocked)"
            }
        );
        for mixin in &scan.mixins {
            let shown: Vec<_> = mixin
                .members
                .iter()
                .filter(|m| all || matches!(m.verdict.name(), "partial" | "blocked"))
                .collect();
            if shown.is_empty() {
                continue;
            }
            let targets = if mixin.targets.is_empty() {
                "?".to_string()
            } else {
                mixin
                    .targets
                    .iter()
                    .map(|t| short(t))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let _ = writeln!(out, "    {} → {targets}", short(&mixin.name));
            for m in shown {
                let at =
                    m.at.as_deref()
                        .map(|a| format!(" at {a}"))
                        .unwrap_or_default();
                let methods = if m.target_methods.is_empty() {
                    String::new()
                } else {
                    format!(" {}", m.target_methods.join(", "))
                };
                let _ = writeln!(out, "      @{}{methods}{at}: {}", m.annotation, m.verdict);
            }
        }
    }

    // What the mod builds on.
    let counts = porter_java::reference_counts(scan);
    if let Some(mc) = counts.get(&SymbolOrigin::Minecraft) {
        section(
            &mut out,
            "Most used Minecraft classes (by number of the mod's classes using them)",
        );
        ranked(&mut out, mc, top);
    }
    let supers = minecraft_superclasses(scan);
    if !supers.is_empty() {
        section(
            &mut out,
            "Minecraft classes the mod extends or implements (what modkit must provide)",
        );
        ranked(&mut out, &supers, top);
    }
    if let Some(api) = counts.get(&SymbolOrigin::LoaderApi) {
        section(&mut out, "Loader API classes used");
        ranked(&mut out, api, top);
    }
    let libraries = library_groups(scan);
    if !libraries.is_empty() {
        section(
            &mut out,
            "Other mods and libraries (need a shim or a port first)",
        );
        for (g, c) in libraries.iter().take(top) {
            let _ = writeln!(out, "  {:>6}  {g}", n(*c));
        }
    }
    if let Some(platform) = counts.get(&SymbolOrigin::Platform) {
        let lwjgl: Vec<_> = platform
            .keys()
            .filter(|k| k.starts_with("org.lwjgl."))
            .collect();
        if !lwjgl.is_empty() {
            section(
                &mut out,
                "Direct LWJGL / OpenGL use (Horizon renders with wgpu)",
            );
            for k in lwjgl.iter().take(top) {
                let _ = writeln!(out, "  {k}");
            }
        }
    }

    // Registries.
    let by_registry = registry_counts(scan);
    if !by_registry.is_empty() {
        section(&mut out, "Registry entries found in code");
        for (r, c) in &by_registry {
            let _ = writeln!(out, "  {:>6}  {r}", n(*c));
        }
        let via: BTreeSet<&str> = scan.registrations.iter().map(|r| r.via.as_str()).collect();
        let _ = writeln!(
            out,
            "  (found through {}; entries built in loops or helpers are missed)",
            via.into_iter().collect::<Vec<_>>().join(", ")
        );
    }

    // Resources.
    if !scan.resources.categories.is_empty() {
        section(
            &mut out,
            "Resources (carried over by Horizon's content-mod path)",
        );
        let mut cats: Vec<_> = scan.resources.categories.iter().collect();
        cats.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (cat, c) in cats.into_iter().take(top) {
            let _ = writeln!(out, "  {:>6}  {cat}", n(*c));
        }
        let _ = writeln!(out, "  from: {}", scan.resources.roots.join(", "));
    }

    if !scan.warnings.is_empty() {
        section(&mut out, "Warnings");
        for w in &scan.warnings {
            let _ = writeln!(out, "  {w}");
        }
    }
    out
}

/// What a report's header says about a mod.
pub(crate) struct Facts {
    /// `Create 6.0.11 (create)`.
    pub title: String,
    /// `NeoForge`, `Fabric + NeoForge`.
    pub loaders: Option<String>,
    pub minecraft: Option<String>,
    pub licence: Option<String>,
    /// `source tree` or `jar`.
    pub input_kind: &'static str,
    /// Mods it needs (not the game, the loader or its API).
    pub dependencies: Vec<String>,
}

pub(crate) fn facts(scan: &ModScan) -> Facts {
    let main = scan
        .main_metadata()
        .filter(|m| m.name.is_some() || m.id.is_some());
    let title = match main {
        Some(m) => {
            let mut t = m.name.clone().or(m.id.clone()).unwrap_or_default();
            // A version the build script computes stays a placeholder: leave it out.
            if let Some(v) = m.version.as_ref().filter(|v| !v.contains('$')) {
                let _ = write!(t, " {v}");
            }
            if let (Some(id), Some(_)) = (&m.id, &m.name) {
                let _ = write!(t, " ({id})");
            }
            t
        }
        None => "Unknown mod".to_string(),
    };
    let loaders: BTreeSet<String> = scan.metadata.iter().map(|m| m.loader.to_string()).collect();
    let dependencies: BTreeSet<String> = scan
        .metadata
        .iter()
        .flat_map(|m| m.dependencies.iter().cloned())
        .collect();
    Facts {
        title,
        loaders: (!loaders.is_empty()).then(|| loaders.into_iter().collect::<Vec<_>>().join(" + ")),
        minecraft: scan.metadata.iter().find_map(|m| m.minecraft.clone()),
        licence: scan.metadata.iter().find_map(|m| m.license.clone()),
        input_kind: match scan.input_kind {
            InputKind::SourceTree => "source tree",
            InputKind::Jar => "jar",
        },
        dependencies: dependencies.into_iter().collect(),
    }
}

fn header(scan: &ModScan, out: &mut String) {
    let f = facts(scan);
    let mut line = vec![f.title];
    line.extend(f.loaders);
    line.extend(f.minecraft.map(|mc| format!("Minecraft {mc}")));
    line.extend(f.licence.map(|l| format!("licence {l}")));
    let _ = writeln!(out, "{}", line.join(" · "));
    let _ = writeln!(out, "Scanned {} ({})\n", scan.input, f.input_kind);
    if !f.dependencies.is_empty() {
        row(out, "Needs mods", &f.dependencies.join(", "));
    }
}

/// Mixin members per verdict, in report order.
pub(crate) fn verdict_tally(scan: &ModScan) -> Vec<(&'static str, usize)> {
    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
    for m in scan.mixins.iter().flat_map(|m| &m.members) {
        *tally.entry(m.verdict.name()).or_default() += 1;
    }
    ["supported", "partial", "blocked", "not needed"]
        .into_iter()
        .map(|k| (k, tally.get(k).copied().unwrap_or(0)))
        .collect()
}

/// Mixin targets outside Minecraft and the mod itself: other mods or the loader.
pub(crate) fn other_mod_targets(scan: &ModScan) -> BTreeSet<&str> {
    let own: BTreeSet<&str> = scan.classes.iter().map(|c| c.name.as_str()).collect();
    scan.mixins
        .iter()
        .flat_map(|m| &m.targets)
        .filter(|t| {
            !own.contains(t.as_str())
                && !matches!(
                    SymbolOrigin::classify(t, |_| false),
                    SymbolOrigin::Minecraft | SymbolOrigin::Jdk | SymbolOrigin::Platform
                )
        })
        .map(String::as_str)
        .collect()
}

pub(crate) fn registry_counts(scan: &ModScan) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &scan.registrations {
        *counts.entry(r.registry.clone()).or_default() += 1;
    }
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

/// Third-party references grouped by package root (three segments), with how many
/// of the mod's classes use each group.
/// The known libraries a mod uses, with what becomes of each in a port.
pub(crate) fn known_libraries(scan: &ModScan) -> Vec<(&'static str, &'static str, usize)> {
    let mut found: BTreeMap<&'static str, (&'static str, BTreeSet<&str>)> = BTreeMap::new();
    for class in &scan.classes {
        for r in &class.references {
            if let Some((name, note)) = porter_map::library(r) {
                found
                    .entry(name)
                    .or_insert((note, BTreeSet::new()))
                    .1
                    .insert(class.name.as_str());
            }
        }
    }
    found
        .into_iter()
        .map(|(name, (note, classes))| (name, note, classes.len()))
        .collect()
}

pub(crate) fn library_groups(scan: &ModScan) -> Vec<(String, usize)> {
    let Some(libs) = porter_java::reference_counts(scan).remove(&SymbolOrigin::Library) else {
        return Vec::new();
    };
    let mut groups: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for class in &scan.classes {
        for r in &class.references {
            if libs.contains_key(r) {
                let group = r
                    .split('.')
                    .take_while(|s| !s.starts_with(|c: char| c.is_ascii_uppercase()) && *s != "*")
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(".");
                groups.entry(group).or_default().insert(class.name.as_str());
            }
        }
    }
    let mut v: Vec<_> = groups.into_iter().map(|(g, s)| (g, s.len())).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

pub(crate) fn minecraft_superclasses(scan: &ModScan) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for class in &scan.classes {
        for parent in class.superclass.iter().chain(&class.interfaces) {
            if SymbolOrigin::classify(parent, |_| false) == SymbolOrigin::Minecraft {
                *out.entry(parent.clone()).or_default() += 1;
            }
        }
    }
    out
}

fn ranked(out: &mut String, counts: &BTreeMap<String, usize>, top: usize) {
    let mut v: Vec<_> = counts.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (name, c) in v.iter().take(top) {
        let _ = writeln!(out, "  {:>6}  {name}", n(**c));
    }
    if v.len() > top {
        let _ = writeln!(out, "     ...  and {} more", n(v.len() - top));
    }
}

fn section(out: &mut String, title: &str) {
    let _ = writeln!(out, "\n{title}");
}

fn row(out: &mut String, label: &str, text: &str) {
    let _ = writeln!(out, "{label:<12} {text}");
}

/// `com.demo.mixin.LevelMixin` → `LevelMixin`.
pub(crate) fn short(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// 12345 → "12,345".
pub(crate) fn n(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn groups_thousands() {
        assert_eq!(super::n(0), "0");
        assert_eq!(super::n(999), "999");
        assert_eq!(super::n(1000), "1,000");
        assert_eq!(super::n(1234567), "1,234,567");
    }
}
