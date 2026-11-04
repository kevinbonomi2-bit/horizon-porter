//! Reads a Java mod: source, loader metadata, Mixin configs, assets and data.
//!
//! [`scan`] takes a mod's source tree (a Gradle project, single- or multi-loader) or
//! a built jar, and returns a [`ModScan`]: every class, Mixin, registry entry and
//! resource it found, with what each one depends on.

mod classfile;
mod jar;
mod metadata;
mod mixin;
mod resources;
mod source;
mod walk;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use porter_model::{
    ClassInfo, InputKind, MixinClass, MixinConfig, ModMetadata, ModScan, Registration, Side,
};

use source::{Annotation, ParsedFile, Value};

/// Scans a mod: a source directory or a `.jar` file.
pub fn scan(input: &Path) -> Result<ModScan> {
    if input.is_dir() {
        scan_tree(input)
    } else if input.is_file() {
        jar::scan(input)
    } else {
        bail!("{} is neither a directory nor a file", input.display())
    }
}

/// Classes in `java.lang`, which Java code uses without importing.
const JAVA_LANG: &[&str] = &[
    "Object",
    "String",
    "Integer",
    "Long",
    "Short",
    "Byte",
    "Character",
    "Boolean",
    "Float",
    "Double",
    "Number",
    "Math",
    "StrictMath",
    "System",
    "Thread",
    "Runnable",
    "Iterable",
    "Comparable",
    "CharSequence",
    "StringBuilder",
    "Enum",
    "Record",
    "Class",
    "Exception",
    "RuntimeException",
    "Error",
    "Throwable",
    "IllegalArgumentException",
    "IllegalStateException",
    "UnsupportedOperationException",
    "Override",
    "Deprecated",
    "SuppressWarnings",
    "FunctionalInterface",
    "SafeVarargs",
    "AutoCloseable",
    "Cloneable",
    "Void",
    "ThreadLocal",
    "Process",
    "Runtime",
];

fn scan_tree(root: &Path) -> Result<ModScan> {
    let files = walk::collect(root)?;
    let mut warnings = Vec::new();

    // Gradle properties, shallowest first: the root project's values win.
    let mut props = HashMap::new();
    let mut prop_files: Vec<_> = files
        .iter()
        .filter(|f| f.name() == "gradle.properties")
        .collect();
    prop_files.sort_by_key(|f| f.rel.matches('/').count());
    for f in prop_files {
        for (k, v) in metadata::gradle_properties(&read_lossy(&f.abs)?) {
            props.entry(k).or_insert(v);
        }
    }

    let mut mods: Vec<ModMetadata> = Vec::new();
    for f in &files {
        let Some(loader) = metadata::loader_for(&f.rel) else {
            continue;
        };
        let text = metadata::substitute(&read_lossy(&f.abs)?, &props);
        match metadata::parse_metadata(loader, &f.rel, &text) {
            Ok(m) => mods.push(m),
            Err(e) => warnings.push(format!("{e:#}")),
        }
    }
    if mods.is_empty() {
        warnings.push(
            "no fabric.mod.json, neoforge.mods.toml or mods.toml found: is this a mod's source tree?"
                .into(),
        );
    }

    let listed: BTreeSet<&str> = mods
        .iter()
        .flat_map(|m| m.mixin_configs.iter().map(String::as_str))
        .collect();
    let mut mixin_configs = Vec::new();
    for f in files
        .iter()
        .filter(|f| metadata::is_mixin_config_name(f.name()) || listed.contains(f.name()))
    {
        if let Ok(c) = metadata::parse_mixin_config(&f.rel, &read_lossy(&f.abs)?) {
            mixin_configs.push(c);
        }
    }

    let resources = resources::summarize(files.iter().map(|f| f.rel.as_str()));

    let mut parser = source::new_parser()?;
    let mut parsed: Vec<(String, ParsedFile)> = Vec::new();
    let mut kotlin = 0;
    for f in &files {
        if f.name().ends_with(".kt") {
            kotlin += 1;
        }
        if !f.name().ends_with(".java")
            || f.name() == "package-info.java"
            || f.name() == "module-info.java"
        {
            continue;
        }
        let file = source::parse(&mut parser, &read_lossy(&f.abs)?)?;
        if file.has_syntax_errors {
            warnings.push(format!(
                "{}: parts of this file could not be parsed; its facts may be incomplete",
                f.rel
            ));
        }
        parsed.push((f.rel.clone(), file));
    }
    if kotlin > 0 {
        warnings.push(format!(
            "{kotlin} Kotlin files were not read: only Java is supported for now"
        ));
    }

    Ok(assemble(
        root,
        mods,
        mixin_configs,
        resources,
        parsed,
        warnings,
    ))
}

/// Builds the scan from parsed files: resolves names across files, then derives
/// classes, Mixins and registrations.
fn assemble(
    root: &Path,
    metadata: Vec<ModMetadata>,
    mixin_configs: Vec<MixinConfig>,
    resources: porter_model::ResourceSummary,
    parsed: Vec<(String, ParsedFile)>,
    mut warnings: Vec<String>,
) -> ModScan {
    let own: BTreeSet<String> = parsed
        .iter()
        .flat_map(|(_, f)| f.types.iter().map(|t| qualify(&f.package, &t.name)))
        .collect();
    let config_sides: HashMap<&str, Side> = mixin_configs
        .iter()
        .flat_map(|c| c.mixins.iter().map(|(n, s)| (n.as_str(), *s)))
        .collect();

    let mut classes = Vec::new();
    let mut mixins = Vec::new();
    for (path, file) in &parsed {
        let resolver = Resolver { file, own: &own };
        let file_refs = resolver.file_references();
        for t in &file.types {
            let name = qualify(&file.package, &t.name);
            let superclass = t.superclass.as_deref().map(|s| resolver.resolve(s));
            let interfaces: Vec<String> =
                t.interfaces.iter().map(|i| resolver.resolve(i)).collect();
            let mixin_annotation = t.annotations.iter().find(|a| a.simple_name() == "Mixin");
            let side = config_sides
                .get(name.as_str())
                .copied()
                .unwrap_or_else(|| side_of(&file.package, &t.annotations));

            let mut references: BTreeSet<String> = file_refs.clone();
            references.extend(superclass.iter().cloned());
            references.extend(interfaces.iter().cloned());
            references.remove(&name);

            if let Some(annotation) = mixin_annotation {
                let mut members = mixin::members(&t.members);
                members.extend(interfaces.iter().map(|i| mixin::interface_member(i)));
                mixins.push(MixinClass {
                    name: name.clone(),
                    targets: mixin_targets(annotation, &resolver),
                    side,
                    added_interfaces: interfaces.clone(),
                    members,
                });
            }
            classes.push(ClassInfo {
                name,
                kind: t.kind,
                path: path.clone(),
                superclass,
                interfaces,
                references: references.into_iter().collect(),
                side,
                is_mixin: mixin_annotation.is_some(),
                lines: if file.types.len() == 1 { file.lines } else { 0 },
                methods: t.methods,
            });
        }
    }

    for config in &mixin_configs {
        for (name, _) in &config.mixins {
            if !own.contains(name) {
                warnings.push(format!(
                    "{} lists {name}, which is not in the source tree",
                    config.path
                ));
            }
        }
    }

    let registrations = registrations(&parsed);
    ModScan {
        input: root.display().to_string(),
        input_kind: InputKind::SourceTree,
        metadata,
        classes,
        mixin_configs,
        mixins,
        registrations,
        resources,
        bundled_jars: Vec::new(),
        warnings,
    }
}

/// Resolves type names as written in one file to fully qualified names.
struct Resolver<'a> {
    file: &'a ParsedFile,
    own: &'a BTreeSet<String>,
}

impl Resolver<'_> {
    fn resolve(&self, written: &str) -> String {
        let written = source::base_type(written);
        if let Some((first, rest)) = written.split_once('.') {
            // Already qualified (`java.io.Serializable`), or Outer.Inner.
            if first.starts_with(|c: char| c.is_ascii_lowercase()) {
                return written;
            }
            return format!("{}.{rest}", self.resolve(first));
        }
        let imports = &self.file.imports;
        if let Some(i) = imports
            .iter()
            .find(|i| !i.is_static && !i.wildcard && last_segment(&i.path) == written)
        {
            return i.path.clone();
        }
        let same_package = qualify(&self.file.package, &written);
        if self.own.contains(&same_package) {
            return same_package;
        }
        if JAVA_LANG.contains(&written.as_str()) {
            return format!("java.lang.{written}");
        }
        let wildcards: Vec<&str> = imports
            .iter()
            .filter(|i| i.wildcard && !i.is_static)
            .map(|i| i.path.as_str())
            .collect();
        if let Some(w) = wildcards
            .iter()
            .find(|w| self.own.contains(&format!("{w}.{written}")))
        {
            return format!("{w}.{written}");
        }
        if let [only] = wildcards.as_slice() {
            return format!("{only}.{written}");
        }
        same_package
    }

    /// Everything the file refers to from outside itself: imports and fully
    /// qualified uses. Wildcard imports are kept as `package.*`.
    fn file_references(&self) -> BTreeSet<String> {
        let mut refs = BTreeSet::new();
        for i in &self.file.imports {
            let r = match (i.is_static, i.wildcard) {
                (false, false) => i.path.clone(),
                (false, true) => format!("{}.*", i.path),
                // `import static a.B.c;` and `import static a.B.*;` both refer to a.B.
                (true, false) => match i.path.rsplit_once('.') {
                    Some((class, _)) => class.to_string(),
                    None => continue,
                },
                (true, true) => i.path.clone(),
            };
            refs.insert(r);
        }
        refs.extend(self.file.qualified_refs.iter().cloned());
        refs
    }
}

fn mixin_targets(annotation: &Annotation, resolver: &Resolver) -> Vec<String> {
    let mut targets = Vec::new();
    for value in annotation
        .arg("value")
        .map(Value::items)
        .unwrap_or_default()
    {
        if let Value::Class(t) = value {
            targets.push(resolver.resolve(t));
        }
    }
    for value in annotation
        .arg("targets")
        .map(Value::items)
        .unwrap_or_default()
    {
        if let Some(s) = value.as_str() {
            targets.push(s.replace(['/', '$'], "."));
        }
    }
    targets
}

fn side_of(package: &str, annotations: &[Annotation]) -> Side {
    for a in annotations {
        if matches!(a.simple_name(), "OnlyIn" | "Environment") {
            let text = a.arg("value").map(Value::text).unwrap_or_default();
            if text.contains("CLIENT") {
                return Side::Client;
            }
            if text.contains("SERVER") {
                return Side::Server;
            }
        }
    }
    if package.split('.').any(|s| s == "client") {
        Side::Client
    } else {
        Side::Common
    }
}

fn registrations(parsed: &[(String, ParsedFile)]) -> Vec<Registration> {
    // DeferredRegister fields by (declaring class simple name, field name).
    let fields: HashMap<(&str, &str), &str> = parsed
        .iter()
        .flat_map(|(_, f)| f.registers.iter())
        .map(|r| ((r.class.as_str(), r.field.as_str()), r.registry.as_str()))
        .collect();
    let mut out = Vec::new();
    for (_, file) in parsed {
        for call in &file.calls {
            let class = qualify(&file.package, &call.enclosing);
            if call.object_is_chain {
                continue;
            }
            let object_lower = call.object.to_ascii_lowercase();
            if let Some((_, registry)) = source::REGISTRATE_METHODS
                .iter()
                .find(|(m, _)| *m == call.method)
                .filter(|_| object_lower.contains("registrate"))
            {
                out.push(Registration {
                    registry: registry.to_string(),
                    id: call.first_string.clone(),
                    via: "Registrate".into(),
                    class,
                });
                continue;
            }
            if call.object == "Registry" || call.object.ends_with(".Registry") {
                let Some(reg) = call.args.first().filter(|a| a.contains("Registries")) else {
                    continue;
                };
                out.push(Registration {
                    registry: last_segment(reg).to_ascii_lowercase(),
                    id: call.second_arg_last_string.clone(),
                    via: "Registry.register".into(),
                    class,
                });
                continue;
            }
            // BLOCKS.register(...) in the declaring class, or ModBlocks.BLOCKS.register(...).
            let key = match call.object.rsplit_once('.') {
                Some((owner, field)) => (last_segment(owner), field),
                None => (call.enclosing.as_str(), call.object.as_str()),
            };
            if let Some(registry) = fields.get(&key) {
                out.push(Registration {
                    registry: registry.to_string(),
                    id: call.first_string.clone(),
                    via: "DeferredRegister".into(),
                    class,
                });
            }
        }
    }
    out
}

fn qualify(package: &str, name: &str) -> String {
    if package.is_empty() {
        name.to_string()
    } else {
        format!("{package}.{name}")
    }
}

fn last_segment(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

fn read_lossy(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    Ok(String::from_utf8(bytes)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

/// Counts of how many of the mod's classes refer to each external class, grouped by
/// where it comes from. Used by reports.
pub fn reference_counts(
    scan: &ModScan,
) -> BTreeMap<porter_model::SymbolOrigin, BTreeMap<String, usize>> {
    let own: BTreeSet<&str> = scan.classes.iter().map(|c| c.name.as_str()).collect();
    let own_packages: BTreeSet<&str> = scan
        .classes
        .iter()
        .filter_map(|c| c.name.rsplit_once('.').map(|(p, _)| p))
        .collect();
    let is_own = |name: &str| {
        // `import com.demo.block.*;` is kept as `com.demo.block.*`.
        if let Some(package) = name.strip_suffix(".*")
            && own_packages.contains(package)
        {
            return true;
        }
        let mut prefix = name;
        loop {
            if own.contains(prefix) {
                return true;
            }
            match prefix.rsplit_once('.') {
                Some((p, _)) => prefix = p,
                None => return false,
            }
        }
    };
    let mut out: BTreeMap<_, BTreeMap<String, usize>> = BTreeMap::new();
    for class in &scan.classes {
        for r in &class.references {
            let origin = porter_model::SymbolOrigin::classify(r, is_own);
            *out.entry(origin).or_default().entry(r.clone()).or_default() += 1;
        }
    }
    out
}
