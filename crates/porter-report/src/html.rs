// The shareable report: one self-contained HTML page (no scripts, no external files)
// that a mod author can open, attach to an issue or post anywhere.

use std::collections::BTreeMap;
use std::fmt::Write;

use porter_map::reach::Portability;
use porter_model::{InputKind, ModScan, Side, SymbolOrigin, Verdict};

use crate::text::{
    facts, library_groups, minecraft_superclasses, n, other_mod_targets, registry_counts, short,
    verdict_tally,
};

/// Entries shown before a list folds the rest into "Show all".
const SHOWN: usize = 15;

/// The full report as an HTML page.
pub fn page(scan: &ModScan) -> String {
    render(scan, false)
}

/// The report for showing inside the desktop app, which has its own header: the
/// same page without the header and footer.
pub fn embedded(scan: &ModScan) -> String {
    render(scan, true)
}

fn render(scan: &ModScan, embedded: bool) -> String {
    let f = facts(scan);
    let mut b = String::new();
    let _ = write!(
        b,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"generator\" content=\"horizon-porter {version}\">\n\
         <title>{title} · horizon-porter report</title>\n<style>{STYLE}</style>\n</head>\n<body{class}>\n<main>\n",
        version = env!("CARGO_PKG_VERSION"),
        title = esc(&f.title),
        class = if embedded { " class=\"embedded\"" } else { "" },
    );

    if !embedded {
        header(scan, &mut b);
    }
    outlook(scan, &mut b);
    cards(scan, &mut b);
    mixins(scan, &mut b);
    builds_on(scan, &mut b);
    registries(scan, &mut b);
    resources(scan, &mut b);

    if !scan.warnings.is_empty() {
        b.push_str("<section>\n<h2>Warnings</h2>\n<ul class=\"warnings\">\n");
        for w in &scan.warnings {
            let _ = writeln!(b, "<li>{}</li>", esc(w));
        }
        b.push_str("</ul>\n</section>\n");
    }

    if embedded {
        b.push_str("</main>\n</body>\n</html>\n");
    } else {
        let _ = write!(
            b,
            "<footer>\n<p>Made with <a href=\"https://github.com/kevinbonomi2-bit/horizon-porter\">horizon-porter</a> {}. \
             A scan reads the mod; it does not change or port it. A Mixin verdict is about the \
             injection kind horizon-mixins supports; the verdict at the top is about what the \
             Mixins change, and whether Horizon can do it.</p>\n</footer>\n</main>\n</body>\n</html>\n",
            env!("CARGO_PKG_VERSION")
        );
    }
    b
}

fn header(scan: &ModScan, b: &mut String) {
    let f = facts(scan);
    let _ = write!(
        b,
        "<header>\n<p class=\"eyebrow\">Horizon porting report</p>\n<h1>{}</h1>\n<p class=\"chips\">",
        esc(&f.title)
    );
    for chip in f
        .loaders
        .iter()
        .cloned()
        .chain(f.minecraft.iter().map(|m| format!("Minecraft {m}")))
        .chain(f.licence.iter().map(|l| format!("Licence: {l}")))
    {
        let _ = write!(b, "<span class=\"chip\">{}</span>", esc(&chip));
    }
    let _ = write!(
        b,
        "</p>\n<p class=\"muted\">Scanned a {}: <code>{}</code></p>\n",
        f.input_kind,
        esc(&scan.input)
    );
    if !f.dependencies.is_empty() {
        let _ = writeln!(
            b,
            "<p>Needs these mods first: {}</p>",
            f.dependencies
                .iter()
                .map(|d| format!("<code>{}</code>", esc(d)))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    b.push_str("</header>\n");
}

/// Whether the mod can be ported today, before the numbers: a Mixin can hook in the
/// same way and still change something Horizon can't (how the game draws).
fn outlook(scan: &ModScan, b: &mut String) {
    let o = porter_map::reach::outlook(scan);
    if o.total() == 0 {
        return;
    }
    let (class, title) = match o.portability {
        Portability::Likely => ("v-ok", "Its Mixins reach Horizon"),
        Portability::Partly => ("v-partial", "Partly portable"),
        Portability::NotYet => ("v-blocked", "Not portable yet"),
    };
    let summary = o
        .summary
        .strip_prefix("Not portable yet. ")
        .or_else(|| o.summary.strip_prefix("Partly portable. "))
        .unwrap_or(&o.summary);
    let _ = write!(
        b,
        "<section class=\"outlook {class}\">\n<h2>{}</h2>\n<p>{}</p>\n<ul class=\"legend\">",
        esc(title),
        esc(summary)
    );
    for (count, label, class) in [
        (o.hook, "through a Horizon hook", "v-ok"),
        (o.horizon, "through Horizon's own code", "v-ok"),
        (o.screen, "redraw screens", "v-blocked"),
        (o.rendering, "change how the game draws", "v-blocked"),
    ] {
        if count > 0 {
            let _ = write!(
                b,
                "<li><span class=\"dot {class}\"></span><strong>{}</strong> {label}</li>",
                n(count)
            );
        }
    }
    b.push_str("</ul>\n</section>\n");
}

fn cards(scan: &ModScan, b: &mut String) {
    let lines: usize = scan.classes.iter().map(|c| c.lines).sum();
    let methods: usize = scan.classes.iter().map(|c| c.methods).sum();
    let client = scan
        .classes
        .iter()
        .filter(|c| c.side == Side::Client)
        .count();
    let members: usize = scan.mixins.iter().map(|m| m.members.len()).sum();

    b.push_str("<section class=\"cards\">\n");
    let mut code_detail = format!("{} methods · {} client-only", n(methods), n(client));
    if lines > 0 {
        code_detail = format!("{} lines · {code_detail}", n(lines));
    }
    card(b, "Classes", &n(scan.classes.len()), &code_detail);
    card(
        b,
        "Mixin members",
        &n(members),
        &format!("in {} Mixin classes", n(scan.mixins.len())),
    );
    let registries = registry_counts(scan);
    let reg_detail = if scan.input_kind == InputKind::Jar {
        "read from source only".to_string()
    } else {
        registries
            .iter()
            .take(3)
            .map(|(r, c)| format!("{} {r}", n(*c)))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    card(
        b,
        "Registrations",
        &n(scan.registrations.len()),
        &reg_detail,
    );
    card(
        b,
        "Resource files",
        &n(scan.resources.assets() + scan.resources.data()),
        &format!(
            "{} assets · {} data",
            n(scan.resources.assets()),
            n(scan.resources.data())
        ),
    );
    b.push_str("</section>\n");
}

fn card(b: &mut String, label: &str, value: &str, detail: &str) {
    let _ = writeln!(
        b,
        "<div class=\"card\"><p class=\"label\">{}</p><p class=\"value\">{}</p><p class=\"muted\">{}</p></div>",
        esc(label),
        esc(value),
        esc(detail)
    );
}

fn mixins(scan: &ModScan, b: &mut String) {
    if scan.mixins.is_empty() {
        return;
    }
    b.push_str("<section>\n<h2>Mixins</h2>\n");
    let tally = verdict_tally(scan);
    let total: usize = tally.iter().map(|(_, c)| c).sum();
    if total > 0 {
        // One inventory slot per member, grouped by verdict; hover names the member.
        let _ = write!(
            b,
            "<div class=\"slots\" role=\"img\" aria-label=\"{} Mixin members by verdict\">",
            n(total)
        );
        for (verdict, _) in &tally {
            for mixin in &scan.mixins {
                for m in mixin
                    .members
                    .iter()
                    .filter(|m| m.verdict.name() == *verdict)
                {
                    let label = if matches!(m.annotation.as_str(), "merged" | "implements") {
                        format!("{} {}", m.annotation, m.member)
                    } else {
                        format!("@{} {}", m.annotation, m.target_methods.join(", "))
                    };
                    let _ = write!(
                        b,
                        "<i class=\"slot v-{}\" title=\"{} · {}: {}\"></i>",
                        css_class(verdict),
                        esc(short(&mixin.name)),
                        esc(label.trim()),
                        verdict
                    );
                }
            }
        }
        b.push_str("</div>\n<ul class=\"legend\">");
        for (verdict, count) in &tally {
            let _ = write!(
                b,
                "<li><span class=\"dot v-{}\"></span><strong>{}</strong> {}</li>",
                css_class(verdict),
                n(*count),
                verdict
            );
        }
        b.push_str("</ul>\n");
        b.push_str(
            "<p class=\"muted\">How each Mixin hooks in. Supported: horizon-mixins has the \
             matching injection kind. Partial: it can be done with limits. Blocked: \
             horizon-mixins cannot express it yet. Not needed: Rust code reaches the target \
             directly. A supported Mixin can still change something Horizon can't do yet, such \
             as how the game draws: the verdict at the top says so.</p>\n",
        );
    }

    // By annotation.
    let mut by_annotation: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
    for m in scan.mixins.iter().flat_map(|m| &m.members) {
        *by_annotation
            .entry(m.annotation.as_str())
            .or_default()
            .entry(m.verdict.name())
            .or_default() += 1;
    }
    if !by_annotation.is_empty() {
        b.push_str(
            "<table>\n<thead><tr><th>Annotation</th><th class=\"num\">Members</th><th>Verdicts</th></tr></thead>\n<tbody>\n",
        );
        for (annotation, verdicts) in &by_annotation {
            let total: usize = verdicts.values().sum();
            let detail = verdicts
                .iter()
                .map(|(v, c)| {
                    format!(
                        "<span class=\"tag v-{}\">{} {}</span>",
                        css_class(v),
                        n(*c),
                        v
                    )
                })
                .collect::<String>();
            let label = if matches!(*annotation, "merged" | "implements") {
                annotation.to_string()
            } else {
                format!("@{annotation}")
            };
            let _ = writeln!(
                b,
                "<tr><td><code>{}</code></td><td class=\"num\">{}</td><td>{detail}</td></tr>",
                esc(&label),
                n(total)
            );
        }
        b.push_str("</tbody>\n</table>\n");
    }

    // What needs work.
    let mut rows = Vec::new();
    for mixin in &scan.mixins {
        for m in &mixin.members {
            if matches!(m.verdict, Verdict::Partial(_) | Verdict::Blocked(_)) {
                rows.push((mixin, m));
            }
        }
    }
    if !rows.is_empty() {
        let _ = write!(
            b,
            "<h3>Members that need work ({})</h3>\n<table>\n<thead><tr><th>Mixin</th><th>Target</th><th>Member</th><th>Verdict</th></tr></thead>\n<tbody>\n",
            n(rows.len())
        );
        for (mixin, m) in rows {
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
            let mut member = if matches!(m.annotation.as_str(), "merged" | "implements") {
                format!("{} {}", m.annotation, m.member)
            } else {
                format!("@{}", m.annotation)
            };
            if !m.target_methods.is_empty() {
                let _ = write!(member, " {}", m.target_methods.join(", "));
            }
            if let Some(at) = &m.at {
                let _ = write!(member, " at {at}");
            }
            let _ = writeln!(
                b,
                "<tr><td title=\"{}\">{}</td><td title=\"{}\">{}</td><td><code>{}</code></td><td><span class=\"tag v-{}\">{}</span> {}</td></tr>",
                esc(&mixin.name),
                breakable(short(&mixin.name)),
                esc(&mixin.targets.join(", ")),
                breakable(&targets),
                breakable(&member),
                css_class(m.verdict.name()),
                m.verdict.name(),
                esc(m.verdict.note().unwrap_or(""))
            );
        }
        b.push_str("</tbody>\n</table>\n");
    }

    let other = other_mod_targets(scan);
    if !other.is_empty() {
        b.push_str(
            "<h3>Mixins into other mods or the loader</h3>\n<p class=\"muted\">Port or replace these first.</p>\n<ul class=\"plain\">\n",
        );
        for t in other {
            let _ = writeln!(b, "<li><code>{}</code></li>", esc(t));
        }
        b.push_str("</ul>\n");
    }
    b.push_str("</section>\n");
}

fn builds_on(scan: &ModScan, b: &mut String) {
    let counts = porter_java::reference_counts(scan);
    b.push_str("<section>\n<h2>What the mod builds on</h2>\n<div class=\"columns\">\n");

    let supers = minecraft_superclasses(scan);
    ranked_list(
        b,
        "Minecraft classes it extends",
        "What modkit must provide for the port.",
        &sorted(&supers),
    );
    if let Some(mc) = counts.get(&SymbolOrigin::Minecraft) {
        ranked_list(
            b,
            "Most used Minecraft classes",
            "By number of the mod's classes using them.",
            &sorted(mc),
        );
    }
    if let Some(api) = counts.get(&SymbolOrigin::LoaderApi) {
        ranked_list(
            b,
            "Loader API classes",
            "Mapped to modkit in a port.",
            &sorted(api),
        );
    }
    let libraries = library_groups(scan);
    if !libraries.is_empty() {
        ranked_list(
            b,
            "Other mods and libraries",
            "Each needs a shim or a port first.",
            &libraries,
        );
    }
    b.push_str("</div>\n");

    if let Some(platform) = counts.get(&SymbolOrigin::Platform) {
        let lwjgl: Vec<&String> = platform
            .keys()
            .filter(|k| k.starts_with("org.lwjgl."))
            .collect();
        if !lwjgl.is_empty() {
            b.push_str(
                "<h3>Direct LWJGL / OpenGL use</h3>\n<p class=\"muted\">Horizon renders with wgpu, so these calls need rewriting.</p>\n<ul class=\"plain\">\n",
            );
            for k in lwjgl {
                let _ = writeln!(b, "<li><code>{}</code></li>", esc(k));
            }
            b.push_str("</ul>\n");
        }
    }
    b.push_str("</section>\n");
}

fn registries(scan: &ModScan, b: &mut String) {
    if scan.registrations.is_empty() {
        return;
    }
    b.push_str("<section>\n<h2>Registrations found in code</h2>\n");
    let counts = registry_counts(scan);
    ranked_table(b, "Registry", &counts);
    let mut ids: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for r in &scan.registrations {
        if let Some(id) = &r.id {
            ids.entry(r.registry.as_str()).or_default().push(id);
        }
    }
    if !ids.is_empty() {
        b.push_str("<details>\n<summary>Every id found, by registry</summary>\n");
        for (registry, list) in ids {
            let _ = writeln!(
                b,
                "<p><strong>{}</strong> ({}): {}</p>",
                esc(registry),
                n(list.len()),
                esc(&list.join(", "))
            );
        }
        b.push_str("</details>\n");
    }
    b.push_str("</section>\n");
}

fn resources(scan: &ModScan, b: &mut String) {
    if scan.resources.categories.is_empty() {
        return;
    }
    b.push_str(
        "<section>\n<h2>Resources</h2>\n<p class=\"muted\">Data and assets carry over through Horizon's content-mod path.</p>\n",
    );
    let mut cats: Vec<(String, usize)> = scan
        .resources
        .categories
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    cats.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked_table(b, "Category", &cats);
    let _ = writeln!(
        b,
        "<p class=\"muted\">Namespaces: {}. From: {}.</p>",
        esc(&scan.resources.namespaces.join(", ")),
        esc(&scan.resources.roots.join(", "))
    );
    b.push_str("</section>\n");
}

/// A titled list of names with counts; entries past [`SHOWN`] fold away.
fn ranked_list(b: &mut String, title: &str, note: &str, entries: &[(String, usize)]) {
    let _ = write!(
        b,
        "<div>\n<h3>{}</h3>\n<p class=\"muted\">{}</p>\n<ol class=\"ranked\">\n",
        esc(title),
        esc(note)
    );
    for (name, count) in entries.iter().take(SHOWN) {
        ranked_item(b, name, *count);
    }
    b.push_str("</ol>\n");
    if entries.len() > SHOWN {
        let _ = write!(
            b,
            "<details>\n<summary>Show all {}</summary>\n<ol class=\"ranked\" start=\"{}\">\n",
            n(entries.len()),
            SHOWN + 1
        );
        for (name, count) in &entries[SHOWN..] {
            ranked_item(b, name, *count);
        }
        b.push_str("</ol>\n</details>\n");
    }
    b.push_str("</div>\n");
}

fn ranked_item(b: &mut String, name: &str, count: usize) {
    let _ = writeln!(
        b,
        "<li><span class=\"count\">{}</span> <code title=\"{}\">{}</code></li>",
        n(count),
        esc(name),
        breakable(name)
    );
}

fn ranked_table(b: &mut String, label: &str, entries: &[(String, usize)]) {
    let _ = write!(
        b,
        "<table class=\"narrow\">\n<thead><tr><th>{}</th><th class=\"num\">Entries</th></tr></thead>\n<tbody>\n",
        esc(label)
    );
    for (name, count) in entries {
        let _ = writeln!(
            b,
            "<tr><td><code>{}</code></td><td class=\"num\">{}</td></tr>",
            esc(name),
            n(*count)
        );
    }
    b.push_str("</tbody>\n</table>\n");
}

fn sorted(counts: &BTreeMap<String, usize>) -> Vec<(String, usize)> {
    let mut v: Vec<(String, usize)> = counts.iter().map(|(k, c)| (k.clone(), *c)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

fn css_class(verdict: &str) -> &'static str {
    match verdict {
        "supported" | "ported" => "ok",
        "partial" | "stubbed" => "partial",
        "blocked" => "blocked",
        _ => "none",
    }
}

/// Escaped text with soft break points between camelCase words and after dots, so
/// long Java names wrap between words: `EntityContraption<wbr>Interaction<wbr>Mixin`.
fn breakable(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut prev: Option<char> = None;
    for c in text.chars() {
        if c.is_ascii_uppercase() && prev.is_some_and(|p| p.is_ascii_lowercase()) {
            out.push_str("<wbr>");
        }
        out.push_str(&esc(c.encode_utf8(&mut [0; 4])));
        if c == '.' {
            out.push_str("<wbr>");
        }
        prev = Some(c);
    }
    out
}

/// Escapes text for HTML content and attribute values.
pub(crate) fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

const STYLE: &str = r#"
/* Graphite ramp, one ember accent, hairlines; after unslop's tokens (CC BY 4.0,
   Damir Sokolovsky). Verdict colors are set once through --v. */
:root {
  color-scheme: light dark;
  --bg: #fafafa; --card: #ffffff; --raised: #f4f4f5;
  --fg: #171717; --fg-2: #525252; --fg-3: #737373;
  --line: rgba(23, 23, 23, 0.08);
  --accent: #b5482a;
  --ok: #16a34a; --partial: #b45309; --blocked: #dc2626; --none: #a3a3a3;
  --slot-hi: rgba(255, 255, 255, 0.55); --slot-lo: rgba(0, 0, 0, 0.18);
  --ease: cubic-bezier(0.22, 1, 0.36, 1);
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #141415; --card: #19191b; --raised: #232325;
    --fg: #ededed; --fg-2: #a1a1a6; --fg-3: #85858b;
    --line: rgba(237, 237, 237, 0.08);
    --accent: #e0714f;
    --ok: #4cc38a; --partial: #d6a24a; --blocked: #e5484d; --none: #5c5c62;
    --slot-hi: rgba(255, 255, 255, 0.10); --slot-lo: rgba(0, 0, 0, 0.45);
  }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg);
  font: 15px/22px system-ui, -apple-system, "Segoe UI Variable", "Segoe UI", Roboto, sans-serif; }
main { max-width: 1120px; margin: 0 auto; padding: 32px 24px 48px; }
body.embedded main { padding: 20px 24px 40px; }
h1 { font-size: 32px; line-height: 1.15; font-weight: 500; margin: 4px 0 12px; }
h2 { font-size: 20px; line-height: 28px; font-weight: 500; margin: 0 0 12px; }
h3 { font-size: 15px; line-height: 22px; font-weight: 500; margin: 20px 0 4px; }
p { margin: 4px 0; }
a { color: var(--accent); text-underline-offset: 2px; }
code { font: 13px/20px ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
  background: var(--raised); padding: 0 4px; border-radius: 4px; overflow-wrap: anywhere; }
.eyebrow { color: var(--fg-3); font-size: 13px; margin: 0; }
.muted { color: var(--fg-3); font-size: 13px; line-height: 20px; }
.chips { display: flex; flex-wrap: wrap; gap: 8px; margin: 8px 0; }
.chip { border: 1px solid var(--line); background: var(--card); border-radius: 6px;
  padding: 0 8px; font-size: 13px; line-height: 24px; color: var(--fg-2); }
header { margin-bottom: 24px; }
section { background: var(--card); border: 1px solid var(--line); border-radius: 16px;
  padding: 24px; margin: 16px 0; }
.cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 12px;
  background: none; border: 0; padding: 0; margin-top: 0; }
.card { background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 16px 20px; }
.card .label { color: var(--fg-2); font-size: 13px; line-height: 20px; margin: 0; }
.card .value { font-size: 28px; line-height: 36px; font-weight: 500; margin: 4px 0;
  font-variant-numeric: tabular-nums; }
.v-ok { --v: var(--ok); } .v-partial { --v: var(--partial); }
.v-blocked { --v: var(--blocked); } .v-none { --v: var(--none); }
/* The signature: one inventory slot per Mixin member. */
.slots { display: flex; flex-wrap: wrap; gap: 2px; margin: 4px 0 12px; }
.slot { display: block; width: 12px; height: 12px; border-radius: 2px; background: var(--v);
  box-shadow: inset 1px 1px 0 var(--slot-hi), inset -1px -1px 0 var(--slot-lo); }
.legend { display: flex; flex-wrap: wrap; gap: 8px 20px; list-style: none; padding: 0; margin: 0 0 8px;
  color: var(--fg-2); font-size: 14px; }
.legend strong { font-weight: 500; color: var(--fg); font-variant-numeric: tabular-nums; }
.dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: 8px;
  background: var(--v); vertical-align: 1px; }
.tag { display: inline-flex; align-items: center; gap: 6px; color: var(--fg-2); font-size: 13px;
  margin: 0 12px 0 0; white-space: nowrap; }
.tag::before { content: ""; width: 8px; height: 8px; border-radius: 50%; background: var(--v); }
table { width: 100%; border-collapse: collapse; margin: 12px 0; font-size: 14px; line-height: 20px; }
table.narrow { width: auto; min-width: 280px; }
th, td { text-align: left; padding: 8px 12px; border-bottom: 1px solid var(--line); vertical-align: top;
  overflow-wrap: break-word; }
th:first-child, td:first-child { padding-left: 0; }
td code { overflow-wrap: normal; }
th { color: var(--fg-3); font-weight: 400; font-size: 13px; }
.num { text-align: right; font-variant-numeric: tabular-nums; }
.columns { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 8px 32px; }
ol.ranked { list-style: none; padding: 0; margin: 8px 0; }
ol.ranked li { display: flex; gap: 12px; align-items: baseline; padding: 2px 0; min-width: 0; }
ol.ranked code { overflow-wrap: normal; }
.count { min-width: 3.5em; text-align: right; color: var(--fg-3); font-size: 13px;
  font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
  font-variant-numeric: tabular-nums; }
ul.plain { padding-left: 20px; }
details { margin: 8px 0; }
summary { cursor: pointer; color: var(--fg-2); }
summary:hover { color: var(--fg); }
summary:focus-visible, a:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 4px; }
.outlook { border-color: var(--v); border-left-width: 4px; }
.outlook h2 { color: var(--v); }
.outlook p { margin: 8px 0 12px; line-height: 22px; }
.warnings li { margin: 4px 0; overflow-wrap: anywhere; color: var(--fg-2); }
footer { color: var(--fg-3); font-size: 13px; line-height: 20px; margin-top: 24px; }
@media (max-width: 760px) {
  table:not(.narrow) { display: block; overflow-x: auto; }
  main, body.embedded main { padding-left: 16px; padding-right: 16px; }
  section { padding: 16px; }
  h1 { font-size: 26px; }
}
"#;
