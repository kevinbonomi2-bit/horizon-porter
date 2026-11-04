// Scans a built mod jar: metadata, Mixin configs, resources, bundled jars and the
// classes, from their class files. Mixin details and registrations need the source.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use porter_model::{ClassInfo, ClassKind, InputKind, MixinClass, ModScan, Side};

use crate::{classfile, metadata, resources};

/// Class files bigger than this are skipped (no mod class is near it).
const MAX_CLASS_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) fn scan(path: &Path) -> Result<ModScan> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).with_context(|| format!("{} is not a jar", path.display()))?;

    let mut names = Vec::new();
    let mut texts: HashMap<String, String> = HashMap::new();
    let mut classes: Vec<classfile::ClassFile> = Vec::new();
    let mut warnings = Vec::new();
    let mut bundled_jars = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name()?.to_string();
        let file_name = name.rsplit('/').next().unwrap_or(&name).to_string();
        if (name.starts_with("META-INF/jars/") || name.starts_with("META-INF/jarjar/"))
            && name.ends_with(".jar")
        {
            bundled_jars.push(name.clone());
        } else if name.ends_with(".class")
            && file_name != "module-info.class"
            && file_name != "package-info.class"
        {
            if entry.size() > MAX_CLASS_BYTES {
                warnings.push(format!("{name}: skipped, too large"));
            } else {
                let mut bytes = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut bytes)?;
                match classfile::parse(&bytes) {
                    Ok(c) => classes.push(c),
                    Err(e) => warnings.push(format!("{name}: {e:#}")),
                }
            }
        } else if metadata::loader_for(&name).is_some()
            || (metadata::is_mixin_config_name(&file_name) && !name.contains('/'))
        {
            let mut text = String::new();
            entry.read_to_string(&mut text)?;
            texts.insert(name.clone(), text);
        }
        names.push(name);
    }

    let empty = HashMap::new();
    let mut mods = Vec::new();
    for (name, text) in &texts {
        if let Some(loader) = metadata::loader_for(name) {
            match metadata::parse_metadata(loader, name, &metadata::substitute(text, &empty)) {
                Ok(m) => mods.push(m),
                Err(e) => warnings.push(format!("{e:#}")),
            }
        }
    }
    mods.sort_by(|a, b| a.path.cmp(&b.path));

    let listed: BTreeSet<&str> = mods
        .iter()
        .flat_map(|m| m.mixin_configs.iter().map(String::as_str))
        .collect();
    let mut mixin_configs = Vec::new();
    for (name, text) in &texts {
        if (metadata::is_mixin_config_name(name) || listed.contains(name.as_str()))
            && let Ok(c) = metadata::parse_mixin_config(name, text)
        {
            mixin_configs.push(c);
        }
    }
    mixin_configs.sort_by(|a, b| a.path.clone().cmp(&b.path));
    let config_sides: HashMap<&str, Side> = mixin_configs
        .iter()
        .flat_map(|c| c.mixins.iter().map(|(n, s)| (n.as_str(), *s)))
        .collect();

    // Nested classes fold into their top-level class, as in a source scan.
    let mut top: BTreeMap<String, ClassInfo> = BTreeMap::new();
    let mut nested_refs: Vec<(String, BTreeSet<String>)> = Vec::new();
    let mut mixin_flags: BTreeSet<String> = BTreeSet::new();
    for c in classes {
        let outer = c.outer.clone();
        if c.has_mixin_annotation {
            mixin_flags.insert(outer.clone());
        }
        if outer != c.name {
            nested_refs.push((outer, c.references));
            continue;
        }
        let kind = if c.access & classfile::ACC_ANNOTATION != 0 {
            ClassKind::Annotation
        } else if c.access & classfile::ACC_INTERFACE != 0 {
            ClassKind::Interface
        } else if c.access & classfile::ACC_ENUM != 0 {
            ClassKind::Enum
        } else if c.super_name.as_deref() == Some("java.lang.Record") {
            ClassKind::Record
        } else {
            ClassKind::Class
        };
        let side = config_sides.get(c.name.as_str()).copied().unwrap_or(
            if c.name.split('.').any(|s| s == "client") {
                Side::Client
            } else {
                Side::Common
            },
        );
        top.insert(
            c.name.clone(),
            ClassInfo {
                path: format!("{}.class", c.name.replace('.', "/")),
                name: c.name,
                kind,
                superclass: c.super_name.filter(|s| s != "java.lang.Object"),
                interfaces: c.interfaces,
                references: c.references.into_iter().collect(),
                side,
                is_mixin: false,
                lines: 0,
                methods: c.methods,
            },
        );
    }
    for (outer, refs) in nested_refs {
        if let Some(info) = top.get_mut(&outer) {
            let mut all: BTreeSet<String> = info.references.drain(..).collect();
            all.extend(refs);
            all.remove(&outer);
            info.references = all.into_iter().collect();
        }
    }

    let mut mixins = Vec::new();
    for info in top.values_mut() {
        if mixin_flags.contains(&info.name) || config_sides.contains_key(info.name.as_str()) {
            info.is_mixin = true;
            mixins.push(MixinClass {
                name: info.name.clone(),
                targets: Vec::new(),
                side: info.side,
                added_interfaces: info.interfaces.clone(),
                members: Vec::new(),
            });
        }
    }
    if !mixins.is_empty() {
        warnings.push(
            "jar scan: Mixin targets and injections are read from source only; scan the source tree for them"
                .into(),
        );
    }
    warnings.push("jar scan: registrations are read from source only".into());

    Ok(ModScan {
        input: path.display().to_string(),
        input_kind: InputKind::Jar,
        metadata: mods,
        classes: top.into_values().collect(),
        mixin_configs,
        mixins,
        registrations: Vec::new(),
        resources: resources::summarize(names.iter().map(String::as_str)),
        bundled_jars,
        warnings,
    })
}
