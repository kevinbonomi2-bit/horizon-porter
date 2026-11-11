//! A mod's network messages and config values, read from its source.
//!
//! Messages are `CustomPacketPayload` records: their components, their channel (from
//! `CustomPacketPayload.Type`), and which way they travel (NeoForge's
//! `playToServer`/`playToClient`, Fabric's `playC2S`/`playS2C` and receivers).
//! Config values are `ModConfigSpec.Builder` (or Forge's `ForgeConfigSpec`) chains:
//! `push`/`pop` sections, `comment`, `define`, `defineInRange`, `defineEnum`.

use porter_model::{ConfigEntry, ConfigKind, CreativeTabInfo, PayloadField, PayloadInfo};

use crate::machines::{balanced, balanced_braces, calls, split_top};

/// The payload record `class` (simple or qualified name), read from its source.
/// `mod_id` stands in for a namespace written as a constant (`MODID`).
pub fn payload(class: &str, source: &str, mod_id: Option<&str>) -> PayloadInfo {
    let simple = class.rsplit('.').next().unwrap_or(class);
    let mut fields = Vec::new();
    if let Some(i) = find_word(source, &format!("record {simple}"))
        && let Some(open) = source[i..].find('(').map(|o| i + o)
        && let Some(components) = balanced(&source[open..])
    {
        for c in split_top(components) {
            let words: Vec<&str> = c
                .split_whitespace()
                .filter(|w| !w.starts_with('@') && *w != "final")
                .collect();
            if let Some((name, ty)) = words.split_last()
                && !ty.is_empty()
            {
                fields.push(PayloadField {
                    name: (*name).to_string(),
                    java_type: ty.join(" "),
                });
            }
        }
    }
    PayloadInfo {
        class: class.to_string(),
        channel: channel(source, mod_id),
        fields,
        to_server: false,
        to_client: false,
        registered_in: Vec::new(),
    }
}

/// The channel a payload's `Type` names: `fromNamespaceAndPath(MODID, "sync")`,
/// `Identifier.of("demo", "sync")`, `ResourceLocation.parse("demo:sync")`.
fn channel(source: &str, mod_id: Option<&str>) -> Option<String> {
    for name in ["fromNamespaceAndPath", "of", "tryBuild"] {
        for args in calls(source, name) {
            let parts = split_top(&args);
            if parts.len() != 2 {
                continue;
            }
            let Some(path) = string_literal(&parts[1]) else {
                continue;
            };
            let ns = string_literal(&parts[0]).or_else(|| mod_id.map(str::to_string))?;
            return Some(format!("{ns}:{path}"));
        }
    }
    for name in ["parse", "withDefaultNamespace", "ResourceLocation"] {
        for args in calls(source, name) {
            if let Some(id) = string_literal(&args) {
                return Some(if id.contains(':') {
                    id
                } else {
                    format!("{}:{id}", mod_id.unwrap_or("minecraft"))
                });
            }
        }
    }
    None
}

/// Marks which way each payload travels, from the registrations in `class`'s
/// `source`, and that `class` registers it.
pub fn directions(payloads: &mut [PayloadInfo], class: &str, source: &str) {
    let registrations: [(&str, bool, bool); 8] = [
        ("playToServer", true, false),
        ("playToClient", false, true),
        ("playBidirectional", true, true),
        ("commonToServer", true, false),
        ("commonToClient", false, true),
        ("commonBidirectional", true, true),
        ("configurationToServer", true, false),
        ("configurationToClient", false, true),
    ];
    for p in payloads.iter_mut() {
        let simple = p.class.rsplit('.').next().unwrap_or(&p.class);
        let named = |args: &str| {
            find_word(args, &format!("{simple}.")).is_some()
                || find_word(args, &format!("{simple}::")).is_some()
        };
        let before = (p.to_server, p.to_client);
        let mut found = false;
        for (call, server, client) in registrations {
            if calls(source, call).iter().any(|a| named(a)) {
                p.to_server |= server;
                p.to_client |= client;
                found = true;
            }
        }
        // Fabric: PayloadTypeRegistry.playC2S().register(...), and the receivers.
        for (marker, server) in [
            ("playC2S()", true),
            ("playS2C()", false),
            ("ServerPlayNetworking.registerGlobalReceiver", true),
            ("ClientPlayNetworking.registerGlobalReceiver", false),
        ] {
            let mut from = 0;
            while let Some(i) = source[from..].find(marker).map(|i| from + i) {
                from = i + marker.len();
                let rest = &source[from..];
                let args = rest
                    .find('(')
                    .and_then(|o| balanced(&rest[o..]))
                    .unwrap_or_default();
                if named(args) {
                    found = true;
                    if server {
                        p.to_server = true;
                    } else {
                        p.to_client = true;
                    }
                }
            }
        }
        if (found || before != (p.to_server, p.to_client))
            && !p.registered_in.iter().any(|c| c == class)
        {
            p.registered_in.push(class.to_string());
        }
    }
}

/// The creative tabs `source` builds: each `CreativeModeTab.builder()` (Fabric's
/// `FabricItemGroup.builder()`) chain up to its `.build()`, with the id it is
/// registered under, its title, its icon and the items `displayItems` accepts.
pub fn creative_tabs(source: &str) -> Vec<CreativeTabInfo> {
    let mut starts: Vec<usize> = ["CreativeModeTab.builder(", "FabricItemGroup.builder("]
        .iter()
        .flat_map(|m| source.match_indices(m).map(|(i, _)| i))
        .collect();
    starts.sort_unstable();
    let mut out = Vec::new();
    for (n, &start) in starts.iter().enumerate() {
        let next = starts.get(n + 1).copied().unwrap_or(source.len());
        let end = source[start..next]
            .find(".build()")
            .map_or(next, |e| start + e);
        let chain = &source[start..end];
        // `TABS.register("mcwroofs", () -> CreativeModeTab.builder()`: the last
        // string literal of the statement it is in.
        let statement = source[..start]
            .rsplit([';', '}'])
            .next()
            .unwrap_or_default();
        let id = statement
            .rsplit('"')
            .nth(1)
            .filter(|_| statement.matches('"').count() >= 2)
            .map(str::to_string);
        let title = calls(chain, "title").first().and_then(|t| {
            calls(t, "translatable")
                .into_iter()
                .chain(calls(t, "literal"))
                .find_map(|a| string_literal(split_top(&a).first()?))
        });
        let icon = calls(chain, "icon").first().and_then(|a| last_constant(a));
        let mut items = Vec::new();
        for a in calls(chain, "accept") {
            if let Some(item) = last_constant(&a)
                && !items.contains(&item)
            {
                items.push(item);
            }
        }
        out.push(CreativeTabInfo {
            id,
            title,
            icon,
            items,
        });
    }
    out
}

/// The last constant named in `text`, lower-cased as an id: `oak_roof` from
/// `(ItemLike)BlockInit.OAK_ROOF.get()`.
fn last_constant(text: &str) -> Option<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .rfind(|w| {
            w.len() >= 2
                && w.chars().any(|c| c.is_ascii_uppercase())
                && w.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        })
        .map(str::to_ascii_lowercase)
}

/// The models a class registers to bake on their own, to draw as parts:
/// `ModelEvent.RegisterStandalone`/`RegisterAdditional`, Flywheel's `PartialModel`.
/// Each is a model id under `models/` (`grind:block/crusher_wheel`).
pub fn standalone_models(source: &str, mod_id: Option<&str>) -> Vec<String> {
    let markers = [
        "RegisterStandalone",
        "RegisterAdditional",
        "PartialModel",
        "StandaloneModel",
    ];
    if !markers.iter().any(|m| source.contains(m)) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut keep = |ns: &str, path: &str| {
        let is_model = ["block/", "item/", "entity/", "models/"]
            .iter()
            .any(|p| path.starts_with(p));
        let id = format!("{ns}:{}", path.trim_start_matches("models/"));
        if is_model && !out.contains(&id) {
            out.push(id);
        }
    };
    for name in ["fromNamespaceAndPath", "of", "tryBuild"] {
        for args in calls(source, name) {
            let parts = split_top(&args);
            if parts.len() != 2 {
                continue;
            }
            if let Some(path) = string_literal(&parts[1]) {
                let ns = string_literal(&parts[0])
                    .or_else(|| mod_id.map(str::to_string))
                    .unwrap_or_else(|| "minecraft".into());
                keep(&ns, &path);
            }
        }
    }
    // `Create.asResource("block/shaft_half")`, `parse("demo:block/wheel")`.
    for name in [
        "asResource",
        "modLoc",
        "rl",
        "parse",
        "withDefaultNamespace",
    ] {
        for args in calls(source, name) {
            let Some(id) = string_literal(&args) else {
                continue;
            };
            match id.split_once(':') {
                Some((ns, path)) => keep(ns, path),
                None if name == "withDefaultNamespace" => keep("minecraft", &id),
                None => keep(mod_id.unwrap_or("minecraft"), &id),
            }
        }
    }
    // A helper of the class's own that adds the folder: `block("shaft_half")`, for
    // `PartialModel block(String path) { return PartialModel.of(asResource("block/" + path)); }`.
    for (helper, prefix) in prefixing_helpers(source) {
        for args in calls(source, &helper) {
            if let Some(path) = string_literal(&args) {
                keep(mod_id.unwrap_or("minecraft"), &format!("{prefix}{path}"));
            }
        }
    }
    out
}

/// A class's one-string functions that put a folder before their argument
/// (`"block/" + path`): (name, folder).
fn prefixing_helpers(source: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut from = 0;
    while let Some(at) = source[from..].find("(String ").map(|i| from + i) {
        from = at + 1;
        let name: String = source[..at]
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let rest = &source[at + "(String ".len()..];
        let Some(close) = rest.find(')') else {
            continue;
        };
        let param = rest[..close].trim();
        let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !word(&name) || !word(param) {
            continue;
        }
        let Some(open) = rest[close..].find('{').map(|i| close + i) else {
            continue;
        };
        if rest[close..open].contains(';') {
            continue;
        }
        let end = rest[open..].find('}').map_or(rest.len(), |i| open + i);
        let body = &rest[open..end];
        let Some(i) = body.find(&format!("\" + {param}")) else {
            continue;
        };
        let Some(quote) = body[..i].rfind('"') else {
            continue;
        };
        let prefix = &body[quote + 1..i];
        if prefix.ends_with('/') && !out.iter().any(|(n, _)| *n == name) {
            out.push((name, prefix.to_string()));
        }
    }
    out
}

/// The config values a class defines, in order. `enums` gives the constants of the
/// mod's own enums by simple name.
pub fn config(class: &str, source: &str, enums: &dyn Fn(&str) -> Vec<String>) -> Vec<ConfigEntry> {
    let mut out = Vec::new();
    if !source.contains("ConfigSpec") {
        return out;
    }
    let mut sections: Vec<String> = Vec::new();
    let mut comment: Option<String> = None;
    let mut from = 0;
    while let Some(dot) = source[from..].find('.').map(|i| from + i) {
        from = dot + 1;
        let rest = &source[from..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let after = rest[name.len()..].trim_start();
        if name.is_empty() || !after.starts_with('(') {
            continue;
        }
        let Some(args) = balanced(after) else {
            continue;
        };
        let parts = split_top(args);
        match name.as_str() {
            "push" => {
                if let Some(s) = parts.first().and_then(|a| string_literal(a)) {
                    sections.extend(s.split('.').map(str::to_string));
                }
            }
            "pop" => {
                let n = parts
                    .first()
                    .and_then(|a| a.trim().parse::<usize>().ok())
                    .unwrap_or(1);
                for _ in 0..n {
                    sections.pop();
                }
            }
            "comment" => {
                let text: Vec<String> = parts.iter().filter_map(|a| string_literal(a)).collect();
                if !text.is_empty() {
                    comment = Some(text.join(" "));
                }
            }
            "define" | "defineInRange" | "defineEnum" => {
                let Some(key) = parts.first().and_then(|a| string_literal(a)) else {
                    continue;
                };
                let Some(default) = parts.get(1).map(|d| d.trim().to_string()) else {
                    continue;
                };
                let path = sections
                    .iter()
                    .cloned()
                    .chain(std::iter::once(key))
                    .collect::<Vec<_>>()
                    .join(".");
                let (kind, default, values) = if name == "defineEnum" {
                    let (ty, constant) = default.rsplit_once('.').unwrap_or(("", &default));
                    let ty = ty.rsplit('.').next().unwrap_or(ty);
                    let mut values = enums(ty);
                    if values.is_empty() {
                        values.push(constant.to_string());
                    }
                    (ConfigKind::Enum, constant.to_string(), values)
                } else if let Some(s) = string_literal(&default) {
                    (ConfigKind::String, s, Vec::new())
                } else if default == "true" || default == "false" {
                    (ConfigKind::Bool, default, Vec::new())
                } else if let Some(n) = number(&default) {
                    n
                } else {
                    (ConfigKind::String, default, Vec::new())
                };
                let (min, max) = if name == "defineInRange" {
                    (
                        parts.get(2).map(|m| number_text(m)),
                        parts.get(3).map(|m| number_text(m)),
                    )
                } else {
                    (None, None)
                };
                out.push(ConfigEntry {
                    class: class.to_string(),
                    path,
                    kind,
                    default,
                    min,
                    max,
                    values,
                    comment: comment.take(),
                });
            }
            _ => {}
        }
    }
    out
}

/// The config values of a class that keeps them as plain fields with defaults: a
/// Cloth/AutoConfig `ConfigData` (`@Config(name = "x")`), an OmegaConfig `Config`, a
/// MidnightLib `MidnightConfig` (`@Entry public static int x = 5;`), or any class named
/// `...Config` made of such fields. A nested settings object (`public Blinds blinds =
/// new Blinds();` with `public static class Blinds { .. }`) is a section: `blinds.x`.
pub fn field_config(class: &str, source: &str) -> Vec<ConfigEntry> {
    let simple = class.rsplit(['.', '$']).next().unwrap_or(class);
    let marked = source.contains("ConfigData")
        || source.contains("@Config(")
        || source.contains("MidnightConfig")
        || source.contains(&format!("class {simple} implements Config"));
    let named =
        simple.ends_with("Config") || simple.ends_with("Configs") || simple.ends_with("Settings");
    if source.contains("ConfigSpec") || !(marked || named) {
        return Vec::new();
    }
    // Nested settings objects: the field that holds each (`blinds` for `Blinds`).
    let mut sections: Vec<(String, String)> = Vec::new();
    for line in source.lines() {
        let t = line.trim();
        if let Some((decl, init)) = t.strip_suffix(';').and_then(|t| t.split_once(" = new "))
            && decl.starts_with("public ")
        {
            let words: Vec<&str> = decl.split_whitespace().collect();
            let ty = init
                .split('(')
                .next()
                .unwrap_or("")
                .rsplit('.')
                .next()
                .unwrap_or("");
            if let Some(name) = words.last()
                && !ty.is_empty()
                && source.contains(&format!("class {ty} "))
            {
                sections.push((ty.to_string(), name.to_string()));
            }
        }
    }
    let mut out = Vec::new();
    // The class each line is in: the innermost `class X {` whose braces are open.
    let mut open: Vec<(String, i32)> = Vec::new();
    let mut depth = 0i32;
    let mut comment: Option<String> = None;
    let mut bounds: (Option<String>, Option<String>) = (None, None);
    for line in source.lines() {
        let t = line.trim();
        if let Some(i) = t.find("class ").filter(|_| t.ends_with('{')) {
            let name: String = t[i + 6..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            open.push((name, depth));
        }
        // An annotation's text: `@Comment("..")`, `@Description("..")`; its bounds.
        if t.starts_with('@') {
            if (t.starts_with("@Comment") || t.starts_with("@Description"))
                && let (Some(i), Some(j)) = (t.find('"'), t.rfind('"'))
                && j > i
            {
                comment = Some(t[i + 1..j].replace("\\\"", "\""));
            }
            for (key, slot) in [("min = ", &mut bounds.0), ("max = ", &mut bounds.1)] {
                if let Some(v) = t.split_once(key).map(|(_, r)| r) {
                    let v: String = v
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                        .collect();
                    if !v.is_empty() {
                        *slot = Some(v);
                    }
                }
            }
        }
        if let Some(entry) = field_entry(class, t) {
            let owner = open.last().map_or("", |(n, _)| n.as_str());
            let section = sections
                .iter()
                .find(|(ty, _)| ty == owner)
                .map(|(_, f)| f.as_str());
            // A field of a nested class no field holds isn't a setting of this config.
            if owner == simple || section.is_some() {
                let mut entry = entry;
                if let Some(section) = section {
                    entry.path = format!("{section}.{}", entry.path);
                }
                entry.comment = comment.take();
                if matches!(
                    entry.kind,
                    ConfigKind::Int | ConfigKind::Long | ConfigKind::Double
                ) {
                    entry.min = bounds.0.take();
                    entry.max = bounds.1.take();
                }
                out.push(entry);
            }
        }
        if !t.starts_with('@') {
            comment = None;
            bounds = (None, None);
        }
        depth += t.matches('{').count() as i32 - t.matches('}').count() as i32;
        while open.last().is_some_and(|(_, d)| depth <= *d) {
            open.pop();
        }
    }
    // A class with one stray public field isn't a config.
    if !marked && out.len() < 2 {
        return Vec::new();
    }
    out
}

/// `public int searchRadius = 6;` as a config value: a public, non-final field of a
/// plain type with a literal default.
fn field_entry(class: &str, line: &str) -> Option<ConfigEntry> {
    let decl = line.strip_suffix(';')?;
    let (head, default) = decl.split_once(" = ")?;
    let words: Vec<&str> = head.split_whitespace().collect();
    if words.first() != Some(&"public") || words.contains(&"final") {
        return None;
    }
    let [.., ty, name] = words.as_slice() else {
        return None;
    };
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let default = default.trim();
    let (kind, default) = match *ty {
        "boolean" if default == "true" || default == "false" => {
            (ConfigKind::Bool, default.to_string())
        }
        "int" | "long" | "float" | "double" => {
            let (kind, text, _) = number(default)?;
            let kind = match *ty {
                "int" => ConfigKind::Int,
                "long" => ConfigKind::Long,
                _ => match kind {
                    ConfigKind::Int | ConfigKind::Long | ConfigKind::Double => ConfigKind::Double,
                    other => other,
                },
            };
            (kind, text)
        }
        "String" => (ConfigKind::String, string_literal(default)?),
        _ => return None,
    };
    Some(ConfigEntry {
        class: class.to_string(),
        path: name.to_string(),
        kind,
        default,
        min: None,
        max: None,
        values: Vec::new(),
        comment: None,
    })
}

/// The constants of `enum name { A, B(1), C; ... }` in `source`.
pub fn enum_constants(name: &str, source: &str) -> Vec<String> {
    let Some(i) = find_word(source, &format!("enum {name}")) else {
        return Vec::new();
    };
    let Some(open) = source[i..].find('{').map(|o| i + o) else {
        return Vec::new();
    };
    let Some(body) = balanced_braces(&source[open..]) else {
        return Vec::new();
    };
    let head = body.split(';').next().unwrap_or(body);
    split_top(head)
        .iter()
        .filter_map(|c| {
            let id: String = c
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            (!id.is_empty()).then_some(id)
        })
        .collect()
}

/// A numeric literal's kind and value: `5` → Int, `5L` → Long, `0.5`/`2.0D` → Double.
fn number(text: &str) -> Option<(ConfigKind, String, Vec<String>)> {
    let t = text.trim();
    if let Some(v) = t.strip_suffix(['L', 'l']) {
        v.parse::<i64>()
            .ok()
            .map(|_| (ConfigKind::Long, v.to_string(), Vec::new()))
    } else if t.parse::<i64>().is_ok() {
        Some((ConfigKind::Int, t.to_string(), Vec::new()))
    } else {
        let v = t.trim_end_matches(['D', 'd', 'F', 'f']);
        v.parse::<f64>()
            .ok()
            .map(|_| (ConfigKind::Double, v.to_string(), Vec::new()))
    }
}

/// A bound as written, without a type suffix (`Integer.MAX_VALUE` stays as is).
fn number_text(text: &str) -> String {
    number(text).map_or_else(|| text.trim().to_string(), |(_, v, _)| v)
}

/// The text of a string literal (several joined with `+` are concatenated).
fn string_literal(text: &str) -> Option<String> {
    let t = text.trim();
    let mut out = String::new();
    for part in t.split('+') {
        let p = part.trim();
        let inner = p.strip_prefix('"')?.strip_suffix('"')?;
        out.push_str(&inner.replace("\\\"", "\"").replace("\\n", "\n"));
    }
    Some(out)
}

/// Where `word` starts in `source`, not inside a longer identifier.
fn find_word(source: &str, word: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = source[from..].find(word).map(|i| from + i) {
        let before = source[..i].chars().next_back();
        let after = source[i + word.len()..].chars().next();
        let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
        let word_end = word.chars().last().is_some_and(ident);
        if !before.is_some_and(ident) && !(word_end && after.is_some_and(ident)) {
            return Some(i);
        }
        from = i + word.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYLOAD: &str = r#"
public record SetSpeed(BlockPos pos, int speed, @NotNull String note) implements CustomPacketPayload {
    public static final Type<SetSpeed> TYPE =
        new Type<>(ResourceLocation.fromNamespaceAndPath(Grind.MODID, "set_speed"));
}
"#;

    #[test]
    fn reads_a_field_config_with_sections() {
        let src = r#"
@Config(name = "beautify")
public class BeautifyConfig implements ConfigData {
   @CollapsibleObject
   public BeautifyConfig.Blinds blinds = new BeautifyConfig.Blinds();
   public static int searchRadius() {
      return Beautify.CONFIG.blinds.searchRadius;
   }
   public static class Blinds {
      @Tooltip
      @BoundedDiscrete(min = 0L, max = 100L)
      @Comment("Searches below")
      public int searchRadius = 6;
      public boolean opensFromRoot = true;
   }
}
"#;
        let c = field_config("a.BeautifyConfig", src);
        assert_eq!(c.len(), 2, "{c:?}");
        assert_eq!(c[0].path, "blinds.searchRadius");
        assert_eq!(c[0].default, "6");
        assert_eq!(c[0].max.as_deref(), Some("100"));
        assert_eq!(c[0].comment.as_deref(), Some("Searches below"));
        assert_eq!(c[1].path, "blinds.opensFromRoot");
        assert!(matches!(c[1].kind, ConfigKind::Bool));
        // Not a config: no marker, one field.
        assert!(
            field_config("a.Thing", "public class Thing {\n   public int x = 1;\n}").is_empty()
        );
    }

    #[test]
    fn finds_creative_tabs() {
        let source = r#"
            public static final DeferredHolder<CreativeModeTab, CreativeModeTab> ROOFS = CREATIVE_TABS.register(
                "mcwroofs",
                () -> CreativeModeTab.builder()
                    .title(Component.translatable("itemGroup.mcwroofs"))
                    .icon(() -> new ItemStack((ItemLike)BlockInit.OAK_ROOF.get()))
                    .displayItems((enabledFeatures, entries) -> {
                        entries.accept((ItemLike)ItemInit.ROOFING_HAMMER.get());
                        entries.accept((ItemLike)BlockInit.OAK_ROOF.get());
                        entries.accept(BlockInit.OAK_ROOF.get());
                    }).build());
            static final ItemGroup OTHER = FabricItemGroup.builder().title(Component.literal("Gems")).build();
        "#;
        let tabs = creative_tabs(source);
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].id.as_deref(), Some("mcwroofs"));
        assert_eq!(tabs[0].title.as_deref(), Some("itemGroup.mcwroofs"));
        assert_eq!(tabs[0].icon.as_deref(), Some("oak_roof"));
        assert_eq!(tabs[0].items, ["roofing_hammer", "oak_roof"]);
        assert_eq!(tabs[1].id, None);
        assert_eq!(tabs[1].title.as_deref(), Some("Gems"));
        assert!(tabs[1].items.is_empty());
    }

    #[test]
    fn finds_models_named_through_a_helper_that_adds_the_folder() {
        let source = r#"
            public class AllPartialModels {
                public static final PartialModel SHAFT_HALF = block("shaft_half"), ARM = entity("arm");
                private static PartialModel block(String path) {
                    return PartialModel.of(Create.asResource("block/" + path));
                }
                private static PartialModel entity(String path) {
                    return PartialModel.of(Create.asResource("entity/" + path));
                }
            }
        "#;
        assert_eq!(
            standalone_models(source, Some("create")),
            ["create:block/shaft_half", "create:entity/arm"]
        );
    }

    #[test]
    fn finds_standalone_models() {
        let source = r#"
            public static void models(ModelEvent.RegisterStandalone event) {
                event.register(StandaloneModelKey.of(ResourceLocation.fromNamespaceAndPath(Grind.ID, "block/crusher_wheel")));
            }
            public static final PartialModel SHAFT = PartialModel.of(Create.asResource("block/shaft_half"));
            String notAModel = ResourceLocation.fromNamespaceAndPath(Grind.ID, "crushing").toString();
        "#;
        assert_eq!(
            standalone_models(source, Some("grind")),
            ["grind:block/crusher_wheel", "grind:block/shaft_half"]
        );
        assert!(standalone_models("no models here", Some("grind")).is_empty());
    }

    #[test]
    fn reads_a_payload_record() {
        let p = payload("net.grind.net.SetSpeed", PAYLOAD, Some("grind"));
        assert_eq!(p.channel.as_deref(), Some("grind:set_speed"));
        let fields: Vec<(&str, &str)> = p
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.java_type.as_str()))
            .collect();
        assert_eq!(
            fields,
            [("pos", "BlockPos"), ("speed", "int"), ("note", "String")]
        );
    }

    #[test]
    fn finds_which_way_payloads_travel() {
        let mut ps = vec![
            payload("SetSpeed", PAYLOAD, Some("grind")),
            PayloadInfo {
                class: "Synced".into(),
                ..Default::default()
            },
        ];
        directions(
            &mut ps,
            "Network",
            r#"registrar.playToServer(SetSpeed.TYPE, SetSpeed.STREAM_CODEC, Handler::speed);
               PayloadTypeRegistry.playS2C().register(Synced.ID, Synced.CODEC);"#,
        );
        assert!(ps[0].to_server && !ps[0].to_client);
        assert!(!ps[1].to_server && ps[1].to_client);
        assert_eq!(ps[0].registered_in, ["Network"]);
    }

    #[test]
    fn reads_config_values_in_their_sections() {
        let source = r#"
public class Config {
    private static final ModConfigSpec.Builder BUILDER = new ModConfigSpec.Builder();
    static {
        BUILDER.push("general");
        public static final ModConfigSpec.BooleanValue LOUD = BUILDER
            .comment("Whether the crusher makes noise")
            .define("loud", true);
        public static final ModConfigSpec.IntValue SPEED = BUILDER
            .defineInRange("speed", 100, 1, 1000);
        BUILDER.push("display");
        MODE = BUILDER.defineEnum("mode", Mode.FAST);
        BUILDER.pop(2);
        RATE = BUILDER.defineInRange("rate", 0.5D, 0.0D, 1.0D);
    }
    enum Mode { SLOW, FAST(2), OFF; Mode() {} Mode(int x) {} }
}
"#;
        let entries = config("Config", source, &|name| enum_constants(name, source));
        let got: Vec<(&str, ConfigKind, &str)> = entries
            .iter()
            .map(|e| (e.path.as_str(), e.kind, e.default.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("general.loud", ConfigKind::Bool, "true"),
                ("general.speed", ConfigKind::Int, "100"),
                ("general.display.mode", ConfigKind::Enum, "FAST"),
                ("rate", ConfigKind::Double, "0.5"),
            ]
        );
        assert_eq!(
            entries[0].comment.as_deref(),
            Some("Whether the crusher makes noise")
        );
        assert_eq!(entries[1].min.as_deref(), Some("1"));
        assert_eq!(entries[1].max.as_deref(), Some("1000"));
        assert_eq!(entries[2].values, ["SLOW", "FAST", "OFF"]);
        assert_eq!(entries[3].max.as_deref(), Some("1.0"));
    }
}
