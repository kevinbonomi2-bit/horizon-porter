// Loader metadata (fabric.mod.json, neoforge.mods.toml, mods.toml), Gradle properties
// and Mixin config files.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use porter_model::{Loader, MixinConfig, ModMetadata, Side};
use serde_json::Value as Json;

/// Ids that are the game, Java or the loader itself, not mods to port first.
const NOT_DEPENDENCIES: &[&str] = &[
    "minecraft",
    "java",
    "fabricloader",
    "fabric",
    "fabric-api",
    "neoforge",
    "forge",
];

/// `key=value` pairs of a `gradle.properties` file.
pub(crate) fn gradle_properties(text: &str) -> HashMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('!'))
        .filter_map(|l| {
            let (k, v) = l.split_once(['=', ':'])?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Replaces `${key}` and `$key` (both forms Gradle's `expand` accepts) with Gradle
/// property values; unknown keys are left as they are.
pub(crate) fn substitute(text: &str, props: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let (key, consumed) = match after.strip_prefix('{') {
            Some(braced) => match braced.find('}') {
                Some(end) => (&braced[..end], end + 2),
                None => ("", 0),
            },
            None => {
                let len = after
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(after.len());
                (&after[..len], len)
            }
        };
        match props.get(key) {
            Some(v) if !key.is_empty() => out.push_str(v),
            _ => out.push_str(&rest[start..start + 1 + consumed]),
        }
        rest = &after[consumed..];
    }
    out.push_str(rest);
    out
}

/// Whether a dependency id is the game, Java, the loader or a part of its API
/// (Fabric API's modules are `fabric-*`), rather than a mod to port first.
fn is_platform(id: &str) -> bool {
    NOT_DEPENDENCIES.contains(&id) || id.starts_with("fabric-") || id.starts_with("quilt")
}

/// The loader a metadata file belongs to, from its path.
pub(crate) fn loader_for(path: &str) -> Option<Loader> {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name {
        "fabric.mod.json" => Some(Loader::Fabric),
        "neoforge.mods.toml" => Some(Loader::NeoForge),
        "mods.toml" if path.contains("META-INF") => Some(Loader::Forge),
        _ => None,
    }
}

/// Parses one metadata file. `text` should already have Gradle properties substituted.
pub(crate) fn parse_metadata(loader: Loader, path: &str, text: &str) -> Result<ModMetadata> {
    match loader {
        Loader::Fabric => fabric(path, text),
        Loader::NeoForge | Loader::Forge => mods_toml(loader, path, text),
    }
}

fn fabric(path: &str, text: &str) -> Result<ModMetadata> {
    let json: Json = serde_json::from_str(text).with_context(|| format!("parsing {path}"))?;
    let string = |key: &str| json.get(key).and_then(Json::as_str).map(str::to_string);
    let license = match json.get("license") {
        Some(Json::String(s)) => Some(s.clone()),
        Some(Json::Array(a)) => Some(
            a.iter()
                .filter_map(Json::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => None,
    };
    let depends = json.get("depends").and_then(Json::as_object);
    let minecraft = depends.and_then(|d| d.get("minecraft")).map(|v| match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    });
    let dependencies = depends
        .map(|d| d.keys().filter(|k| !is_platform(k)).cloned().collect())
        .unwrap_or_default();
    let mixin_configs = json
        .get("mixins")
        .and_then(Json::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| match m {
                    Json::String(s) => Some(s.clone()),
                    Json::Object(o) => o.get("config").and_then(Json::as_str).map(str::to_string),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ModMetadata {
        loader: Loader::Fabric,
        path: path.to_string(),
        id: string("id"),
        name: string("name"),
        version: string("version"),
        license,
        minecraft,
        dependencies,
        mixin_configs,
    })
}

fn mods_toml(loader: Loader, path: &str, text: &str) -> Result<ModMetadata> {
    let doc: toml::Table = toml::from_str(text).with_context(|| format!("parsing {path}"))?;
    let first_mod = doc
        .get("mods")
        .and_then(|m| m.as_array())
        .and_then(|a| a.first())
        .and_then(|m| m.as_table());
    let mod_string = |key: &str| {
        first_mod
            .and_then(|m| m.get(key))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let id = mod_string("modId");

    // [[dependencies.<modid>]] entries: one array per mod id in the file.
    let mut minecraft = None;
    let mut dependencies = Vec::new();
    if let Some(deps) = doc.get("dependencies").and_then(|d| d.as_table()) {
        for list in deps.values().filter_map(|v| v.as_array()) {
            for dep in list.iter().filter_map(|d| d.as_table()) {
                let Some(dep_id) = dep.get("modId").and_then(|v| v.as_str()) else {
                    continue;
                };
                let optional = dep.get("type").and_then(|v| v.as_str()) == Some("optional")
                    || dep.get("mandatory").and_then(|v| v.as_bool()) == Some(false);
                if dep_id == "minecraft" {
                    minecraft = dep
                        .get("versionRange")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                } else if !optional
                    && !is_platform(dep_id)
                    && !dependencies.iter().any(|d| d == dep_id)
                {
                    dependencies.push(dep_id.to_string());
                }
            }
        }
    }

    let mixin_configs = doc
        .get("mixins")
        .and_then(|m| m.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|m| m.get("config").and_then(|c| c.as_str()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    Ok(ModMetadata {
        loader,
        path: path.to_string(),
        id,
        name: mod_string("displayName"),
        version: mod_string("version"),
        license: doc
            .get("license")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        minecraft,
        dependencies,
        mixin_configs,
    })
}

/// A multi-loader project's metadata files can name each other's mod id (Botania's
/// Fabric and Forge files both say `botania`): a mod is not its own dependency.
pub(crate) fn drop_own_dependencies(mods: &mut [ModMetadata]) {
    let ids: Vec<String> = mods.iter().filter_map(|m| m.id.clone()).collect();
    for m in mods {
        m.dependencies.retain(|d| !ids.contains(d));
    }
}

/// Whether a file name looks like a Mixin config.
pub(crate) fn is_mixin_config_name(name: &str) -> bool {
    name.ends_with(".json") && name.contains("mixins")
}

/// Parses a Mixin config. Fails when the JSON has no `package`, which every Mixin
/// config has.
pub(crate) fn parse_mixin_config(path: &str, text: &str) -> Result<MixinConfig> {
    let json: Json = serde_json::from_str(text).with_context(|| format!("parsing {path}"))?;
    let Some(package) = json.get("package").and_then(Json::as_str) else {
        bail!("{path} has no \"package\": not a Mixin config");
    };
    let mut mixins = Vec::new();
    for (key, side) in [
        ("mixins", Side::Common),
        ("client", Side::Client),
        ("server", Side::Server),
    ] {
        for name in json
            .get(key)
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
        {
            let fqcn = if package.is_empty() {
                name.to_string()
            } else {
                format!("{package}.{name}")
            };
            mixins.push((fqcn, side));
        }
    }
    Ok(MixinConfig {
        path: path.to_string(),
        package: Some(package.to_string()),
        mixins,
        plugin: json
            .get("plugin")
            .and_then(Json::as_str)
            .map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_known_properties_only() {
        let props = gradle_properties("mod_id=demo\n# comment\nmod_version = 1.2\n");
        assert_eq!(
            substitute("${mod_id} ${mod_version} ${other}", &props),
            "demo 1.2 ${other}"
        );
        assert_eq!(
            substitute("\"$mod_version\" $other $ ${unclosed", &props),
            "\"1.2\" $other $ ${unclosed"
        );
    }

    #[test]
    fn reads_fabric_metadata() {
        let m = fabric(
            "src/main/resources/fabric.mod.json",
            r#"{"id":"demo","version":"1.0","license":"MIT",
                "depends":{"minecraft":"~26.3","fabric-api":"*","fabric-screen-api-v1":"*","cloth-config":">=1"},
                "mixins":["demo.mixins.json",{"config":"demo.client.mixins.json","environment":"client"}]}"#,
        )
        .unwrap();
        assert_eq!(m.id.as_deref(), Some("demo"));
        assert_eq!(m.minecraft.as_deref(), Some("~26.3"));
        assert_eq!(m.dependencies, vec!["cloth-config"]);
        assert_eq!(m.mixin_configs.len(), 2);
    }

    #[test]
    fn reads_neoforge_metadata() {
        let m = mods_toml(
            Loader::NeoForge,
            "META-INF/neoforge.mods.toml",
            r#"
license = "MIT"
[[mods]]
modId = "demo"
version = "2.0"
displayName = "Demo"
[[mixins]]
config = "demo.mixins.json"
[[dependencies.demo]]
modId = "minecraft"
type = "required"
versionRange = "[26.3]"
[[dependencies.demo]]
modId = "flywheel"
type = "required"
versionRange = "[1.0,)"
[[dependencies.demo]]
modId = "jei"
type = "optional"
versionRange = "*"
"#,
        )
        .unwrap();
        assert_eq!(m.name.as_deref(), Some("Demo"));
        assert_eq!(m.license.as_deref(), Some("MIT"));
        assert_eq!(m.minecraft.as_deref(), Some("[26.3]"));
        assert_eq!(m.dependencies, vec!["flywheel"]);
        assert_eq!(m.mixin_configs, vec!["demo.mixins.json"]);
    }

    #[test]
    fn reads_mixin_config_sides() {
        let c = parse_mixin_config(
            "demo.mixins.json",
            r#"{"package":"com.demo.mixin","mixins":["LevelMixin"],"client":["client.HudMixin"]}"#,
        )
        .unwrap();
        assert_eq!(
            c.mixins,
            vec![
                ("com.demo.mixin.LevelMixin".to_string(), Side::Common),
                ("com.demo.mixin.client.HudMixin".to_string(), Side::Client),
            ]
        );
    }
}
