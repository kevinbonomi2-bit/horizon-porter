//! Reads a Java mod: source, loader metadata, Mixin configs, assets and data.
//!
//! [`scan`] takes a mod's source tree (a Gradle project, single- or multi-loader) or
//! a built jar, and returns a [`ModScan`]: every class, Mixin, registry entry and
//! resource it found, with what each one depends on.

mod classfile;
pub mod decompile;
mod jar;
mod machines;
mod metadata;
mod mixin;
mod netconf;
mod resources;
mod source;
mod walk;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use porter_model::{
    ClassInfo, ClientFeatures, InputKind, KeyBinding, MixinClass, MixinConfig, ModMetadata,
    ModScan, Registration, Side,
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
    metadata::drop_own_dependencies(&mut mods);
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
        if let Some(line) = file.syntax_error_line {
            warnings.push(format!(
                "{}:{line}: Java syntax the parser could not read; facts from this file may be incomplete",
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

    let mut scan = assemble(root, mods, mixin_configs, resources, parsed, warnings);
    machine_guis(root, &mut scan);
    network_and_config(root, &mut scan);
    Ok(scan)
}

/// The mod's network messages and config values, read from its source files.
fn network_and_config(root: &Path, scan: &mut ModScan) {
    let mod_id = scan.main_metadata().and_then(|m| m.id.clone());
    let sources: Vec<(String, String)> = scan
        .classes
        .iter()
        .filter(|c| !c.is_mixin)
        .filter_map(|c| {
            read_lossy(&root.join(&c.path))
                .ok()
                .map(|t| (c.name.clone(), t))
        })
        .collect();
    let enums = |name: &str| {
        sources
            .iter()
            .map(|(_, text)| netconf::enum_constants(name, text))
            .find(|v| !v.is_empty())
            .unwrap_or_default()
    };
    for class in &scan.classes {
        let is_payload = class
            .interfaces
            .iter()
            .any(|i| i.rsplit('.').next() == Some("CustomPacketPayload"));
        if !is_payload {
            continue;
        }
        if let Some((_, text)) = sources.iter().find(|(name, _)| *name == class.name) {
            scan.payloads
                .push(netconf::payload(&class.name, text, mod_id.as_deref()));
        }
    }
    if !scan.payloads.is_empty() {
        for (name, text) in &sources {
            netconf::directions(&mut scan.payloads, name, text);
        }
        // Registered somewhere the scan can't see: assume both ways.
        for p in &mut scan.payloads {
            if !p.to_server && !p.to_client {
                p.to_server = true;
                p.to_client = true;
            }
        }
    }
    // Screens of the mod's own (`extends Screen`), and who opens them.
    for class in &scan.classes {
        let sup = class.superclass.as_deref().unwrap_or_default();
        if sup.rsplit('.').next() != Some("Screen") {
            continue;
        }
        let simple = class.name.rsplit('.').next().unwrap_or(&class.name);
        let opened_by = sources
            .iter()
            .filter(|(name, text)| {
                *name != class.name
                    && text.contains("setScreen")
                    && text.contains(&format!("new {simple}("))
            })
            .map(|(name, _)| name.clone())
            .collect();
        scan.custom_screens.push(porter_model::CustomScreenInfo {
            class: class.name.clone(),
            opened_by,
        });
    }
    // Models baked to draw as parts.
    for (_, text) in &sources {
        for model in netconf::standalone_models(text, mod_id.as_deref()) {
            if !scan.standalone_models.contains(&model) {
                scan.standalone_models.push(model);
            }
        }
    }
    for (name, text) in &sources {
        scan.config.extend(netconf::config(name, text, &enums));
        if let Some(r) = machines::renderer(name, text) {
            scan.renderers.push(r);
        }
        if let Some(s) = machines::storage(name, text) {
            scan.storages.push(s);
        }
        let superclass = scan
            .classes
            .iter()
            .find(|c| c.name == *name)
            .and_then(|c| c.superclass.as_deref());
        if let Some(s) = machines::block_shape(name, superclass, text) {
            scan.block_shapes.push(s);
        }
        if let Some(m) = machines::mob(name, text) {
            scan.mobs.push(m);
        }
        if let Some((category, color)) = machines::effect(text) {
            scan.effects.push((name.clone(), category, color));
        }
    }
}

/// The mod's container menus and screens, read from their source files.
fn machine_guis(root: &Path, scan: &mut ModScan) {
    for class in &scan.classes {
        let Some(sup) = class.superclass.as_deref() else {
            continue;
        };
        let simple = sup.rsplit('.').next().unwrap_or(sup);
        let is_menu = simple.ends_with("ContainerMenu") || simple.ends_with("ScreenHandler");
        let is_screen = simple.ends_with("ContainerScreen") || simple == "HandledScreen";
        if !is_menu && !is_screen {
            continue;
        }
        let Ok(text) = read_lossy(&root.join(&class.path)) else {
            continue;
        };
        if is_menu {
            scan.menus.push(machines::menu(&class.name, &text));
        } else {
            scan.screens.push(machines::screen(&class.name, &text));
        }
    }
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
                fields: t
                    .members
                    .iter()
                    .filter(|m| !m.is_method)
                    .map(|m| porter_model::FieldInfo {
                        name: m.name.clone(),
                        ty: m.ty.clone(),
                        is_static: m.is_static,
                        is_final: m.is_final,
                        init: m.init.clone(),
                    })
                    .collect(),
                method_names: t
                    .members
                    .iter()
                    .filter(|m| m.is_method && !m.is_constructor)
                    .map(|m| m.name.clone())
                    .collect(),
                saved: saved_values(file, &t.name),
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
    let client = client_features(&parsed);
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
        menus: Vec::new(),
        screens: Vec::new(),
        renderers: Vec::new(),
        custom_screens: Vec::new(),
        standalone_models: Vec::new(),
        storages: Vec::new(),
        block_shapes: Vec::new(),
        mobs: Vec::new(),
        effects: Vec::new(),
        payloads: Vec::new(),
        config: Vec::new(),
        client,
        warnings,
    }
}

/// Client APIs whose use marks a file's `literal("...")` registrations as client
/// commands rather than server commands.
const CLIENT_COMMAND_APIS: &[&str] = &[
    "net.fabricmc.fabric.api.client.command",
    "net.neoforged.neoforge.client.event.RegisterClientCommandsEvent",
];

/// Key bindings and client commands, from the parsed files.
fn client_features(parsed: &[(String, ParsedFile)]) -> ClientFeatures {
    let mut features = ClientFeatures::default();
    for (_, file) in parsed {
        for (args, enclosing) in &file.key_mappings {
            // new KeyMapping("key.demo.zoom", [InputConstants.Type.KEYSYM,] GLFW.GLFW_KEY_Z, "category")
            let Some(name) = args.first().and_then(|a| string_literal(a)) else {
                continue;
            };
            let default_key = args
                .iter()
                .find_map(|a| porter_map::glfw_key_name(a.trim()))
                .unwrap_or_else(|| "key.keyboard.unknown".to_string());
            if !features.keys.iter().any(|k| k.name == name) {
                features.keys.push(KeyBinding {
                    name,
                    default_key,
                    class: qualify(&file.package, enclosing),
                });
            }
        }

        let client_file = file.imports.iter().any(|i| {
            CLIENT_COMMAND_APIS
                .iter()
                .any(|api| i.path.starts_with(api))
        });
        if !client_file {
            continue;
        }
        // dispatcher.register(ClientCommandManager.literal("zoom")...)
        for call in file.calls.iter().filter(|c| c.method == "register") {
            if let Some(name) = call.args.first().and_then(|a| root_literal(a))
                && !features.commands.contains(&name)
            {
                features.commands.push(name);
            }
        }
    }
    features
}

/// `"key.demo.zoom"` → `key.demo.zoom`, when the text is one string literal.
fn string_literal(text: &str) -> Option<String> {
    let inner = text.trim().strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains('"')).then(|| inner.to_string())
}

/// The command name of a Brigadier root, `literal("zoom")` or
/// `ClientCommandManager.literal("zoom").then(...)`: `zoom`.
fn root_literal(arg: &str) -> Option<String> {
    let arg = arg.trim_start();
    let at = arg.find("literal(")?;
    let qualifier = &arg[..at];
    if !qualifier
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        return None;
    }
    let rest = arg[at + "literal(".len()..]
        .trim_start()
        .strip_prefix('"')?;
    let name = &rest[..rest.find('"')?];
    (!name.is_empty()).then(|| name.to_string())
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

/// A registration made directly by a call: Registrate, `Registry.register` or a
/// DeferredRegister.
struct DirectRegistration {
    registry: String,
    via: &'static str,
    id: Option<String>,
    /// The argument that carries the id: 0, or 1 for `Registry.register`.
    id_arg: usize,
}

fn direct_registration(
    call: &source::Call,
    method: Option<&source::MethodSig>,
    fields: &HashMap<(&str, &str), &str>,
) -> Option<DirectRegistration> {
    if call.object_is_chain {
        return None;
    }
    // Multi-loader mods pass a registration callback around:
    // `void registerBlocks(BiConsumer<Block, ResourceLocation> r) { r.accept(block, id); }`
    if call.method == "accept"
        && let Some(m) = method
        && let Some(i) = m.params.iter().position(|p| *p == call.object)
        && m.param_types[i].trim_start().starts_with("BiConsumer")
        && second_type_argument(&m.param_types[i])
            .is_some_and(|t| t.ends_with("ResourceLocation") || t.ends_with("Identifier"))
    {
        return Some(DirectRegistration {
            registry: source::first_type_argument_registry(&m.param_types[i])?,
            via: "BiConsumer",
            id: call.second_arg_last_string.clone(),
            id_arg: 1,
        });
    }
    if let Some((_, registry)) = source::REGISTRATE_METHODS
        .iter()
        .find(|(m, _)| *m == call.method)
        .filter(|_| call.object.to_ascii_lowercase().contains("registrate"))
    {
        return Some(DirectRegistration {
            registry: registry.to_string(),
            via: "Registrate",
            id: call.first_string.clone(),
            id_arg: 0,
        });
    }
    if (call.object == "Registry" || call.object.ends_with(".Registry"))
        && ["register", "registerForHolder", "registerReference"].contains(&call.method.as_str())
    {
        let reg = call.args.first().filter(|a| a.contains("Registries"))?;
        return Some(DirectRegistration {
            registry: last_segment(reg).to_ascii_lowercase(),
            via: "Registry.register",
            id: call.second_arg_last_string.clone(),
            id_arg: 1,
        });
    }
    if !call.method.starts_with("register") {
        return None;
    }
    // BLOCKS.register(...) in the declaring class, or ModBlocks.BLOCKS.register(...).
    let key = match call.object.rsplit_once('.') {
        Some((owner, field)) => (last_segment(owner), field),
        None => (call.enclosing.as_str(), call.object.as_str()),
    };
    fields.get(&key).map(|registry| DirectRegistration {
        registry: registry.to_string(),
        via: "DeferredRegister",
        id: call.first_string.clone(),
        id_arg: 0,
    })
}

/// `BiConsumer<Map<A, B>, ResourceLocation>` → `ResourceLocation`: the second type
/// argument at the top nesting level.
fn second_type_argument(ty: &str) -> Option<&str> {
    let start = ty.find('<')? + 1;
    let mut depth = 0;
    for (i, c) in ty[start..].char_indices() {
        match c {
            '<' => depth += 1,
            '>' if depth == 0 => return None,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                let rest = &ty[start + i + 1..];
                let end = rest.find([',', '>']).unwrap_or(rest.len());
                return Some(rest[..end].trim());
            }
            _ => {}
        }
    }
    None
}

/// A helper method that registers under an id it receives as a parameter, such as
/// Create's `ingredient(String name)` calling `REGISTRATE.item(name, ...)`.
#[derive(Clone)]
struct Helper {
    registry: String,
    param: usize,
}

/// Which parameter of the enclosing method an id argument is built from, if any:
/// `name`, `id.getPath()`, `prefix(name)` and `name + "_slab"` all use `name`.
/// Text inside string literals does not count.
fn id_param(arg: &str, params: &[String]) -> Option<usize> {
    let mut code = String::with_capacity(arg.len());
    let mut in_string = false;
    let mut escaped = false;
    for c in arg.chars() {
        if in_string {
            match (escaped, c) {
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => escaped = false,
            }
        } else if c == '"' {
            in_string = true;
            code.push(' ');
        } else {
            code.push(c);
        }
    }
    code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
        .filter(|t| !t.is_empty())
        .find_map(|t| params.iter().position(|p| p == t))
}

/// The helper a call goes to, as (class simple name, method name), when it is an
/// unqualified, `this.` or `Class.` call.
fn helper_key(call: &source::Call) -> Option<(&str, &str)> {
    let class = match call.object.as_str() {
        "" | "this" => call.enclosing.as_str(),
        o if o.starts_with(|c: char| c.is_ascii_uppercase()) && !o.contains(['(', ' ']) => {
            last_segment(o)
        }
        _ => return None,
    };
    Some((class, call.method.as_str()))
}

fn registrations(parsed: &[(String, ParsedFile)]) -> Vec<Registration> {
    // DeferredRegister fields by (declaring class simple name, field name).
    let fields: HashMap<(&str, &str), &str> = parsed
        .iter()
        .flat_map(|(_, f)| f.registers.iter())
        .map(|r| ((r.class.as_str(), r.field.as_str()), r.registry.as_str()))
        .collect();
    fn enclosing_method<'a>(
        file: &'a ParsedFile,
        call: &source::Call,
    ) -> Option<&'a source::MethodSig> {
        call.in_method.map(|i| &file.methods[i])
    }

    // 1. Helpers: methods that register under an id parameter, directly or by
    //    passing the parameter on to another helper. A few rounds reach helpers of
    //    helpers.
    let mut helpers: HashMap<(String, String), Helper> = HashMap::new();
    for round in 0..4 {
        let before = helpers.len();
        for (_, file) in parsed {
            for call in &file.calls {
                let Some(method) = enclosing_method(file, call) else {
                    continue;
                };
                let found = if round == 0 {
                    direct_registration(call, Some(method), &fields)
                        .filter(|d| d.id.is_none())
                        .and_then(|d| {
                            let arg = call.args.get(d.id_arg)?;
                            let param = id_param(arg, &method.params)?;
                            Some(Helper {
                                registry: d.registry,
                                param,
                            })
                        })
                } else {
                    helper_key(call)
                        .and_then(|(c, m)| helpers.get(&(c.to_string(), m.to_string())))
                        .filter(|h| call.literal_args.get(h.param).is_none_or(Option::is_none))
                        .and_then(|h| {
                            let arg = call.args.get(h.param)?;
                            let param = id_param(arg, &method.params)?;
                            Some(Helper {
                                registry: h.registry.clone(),
                                param,
                            })
                        })
                };
                if let Some(helper) = found {
                    helpers
                        .entry((call.enclosing.clone(), method.name.clone()))
                        .or_insert(helper);
                }
            }
        }
        if helpers.len() == before && round > 0 {
            break;
        }
    }

    // 2. Registrations: direct ones, and helper calls with a literal id. Calls that
    //    only pass an id on are part of a helper, not registrations of their own.
    let mut out = Vec::new();
    let mut helper_used: BTreeSet<(String, String)> = BTreeSet::new();
    for (_, file) in parsed {
        for call in &file.calls {
            let class = qualify(&file.package, &call.enclosing);
            let method = enclosing_method(file, call);
            let passes_param = |arg: Option<&String>| {
                method.is_some_and(|m| arg.and_then(|a| id_param(a, &m.params)).is_some())
            };
            if let Some(d) = direct_registration(call, method, &fields) {
                if d.id.is_none() && passes_param(call.args.get(d.id_arg)) {
                    continue;
                }
                // Registrate sets a block's properties in the chain after the call.
                let mut args = call.args.join(", ");
                if d.via == "Registrate" && !call.chain.is_empty() {
                    args.push(' ');
                    args.push_str(&call.chain);
                }
                out.push(Registration {
                    registry: d.registry,
                    id: d.id,
                    via: d.via.into(),
                    class,
                    creates: created_class(&call.args),
                    args,
                });
                continue;
            }
            let Some((c, m)) = helper_key(call) else {
                continue;
            };
            let key = (c.to_string(), m.to_string());
            let Some(helper) = helpers.get(&key) else {
                continue;
            };
            helper_used.insert(key.clone());
            let id = call.literal_args.get(helper.param).cloned().flatten();
            if id.is_none() && passes_param(call.args.get(helper.param)) {
                continue;
            }
            out.push(Registration {
                registry: helper.registry.clone(),
                id,
                via: "helper method".into(),
                class,
                creates: created_class(&call.args),
                args: call.args.join(", "),
            });
        }
    }
    // 3. A mod's own wrapper the steps above can't follow (a generic
    //    `GameRegistry<T>` around a DeferredRegister, as multi-loader mods have):
    //    `blockWithItem("trim", TrimBlock::new, ...)` registers what `TrimBlock` is,
    //    judged by the vanilla class it extends.
    let supers: HashMap<&str, &str> = parsed
        .iter()
        .flat_map(|(_, f)| f.types.iter())
        .filter_map(|t| {
            let sup = t.superclass.as_deref()?;
            let sup = sup.split('<').next().unwrap_or(sup);
            Some((t.name.as_str(), sup.rsplit('.').next().unwrap_or(sup)))
        })
        .collect();
    let kind_of = |class: &str| -> Option<&'static str> {
        let mut name = class;
        for _ in 0..12 {
            match supers.get(name) {
                Some(sup) if *sup != name => name = sup,
                _ => break,
            }
        }
        if name.ends_with("BlockEntity") {
            Some("block_entity_type")
        } else if name.ends_with("Block") {
            Some("block")
        } else if name.ends_with("Item") {
            Some("item")
        } else if name.ends_with("ContainerMenu") || name == "AbstractContainerMenu" {
            Some("menu")
        } else if name.ends_with("Mob")
            || [
                "Monster",
                "Animal",
                "AmbientCreature",
                "WaterAnimal",
                "LivingEntity",
                "AgeableMob",
                "TamableAnimal",
                "AbstractFish",
                "AbstractGolem",
                "Raider",
                "AbstractVillager",
                "AbstractSkeleton",
                "Zombie",
                "Spider",
                "Slime",
            ]
            .contains(&name)
        {
            // A mob: what `EntityType.Builder.of(EntityFirefly::new, ...)` makes.
            Some("entity_type")
        } else {
            None
        }
    };
    // Ids kept in constants (`CABBAGE_CRATE = id("cabbage_crate")`): a static
    // field's initializer's literals that read as ids, by `Class.FIELD` and, when
    // no other class has a field of that name, by `FIELD`. A constant with two
    // (`CABBAGE_CROP = id("cabbages", "cabbage_seeds")`) names a block, then its
    // item.
    let mut constants: HashMap<String, Vec<String>> = HashMap::new();
    let mut bare: HashMap<String, Option<Vec<String>>> = HashMap::new();
    for (_, file) in parsed {
        for t in &file.types {
            for m in t.members.iter().filter(|m| !m.is_method && m.is_static) {
                let Some(init) = m.init.as_deref() else {
                    continue;
                };
                let ids: Vec<String> = init
                    .split('"')
                    .skip(1)
                    .step_by(2)
                    .filter(|l| valid_id(l))
                    .map(str::to_string)
                    .collect();
                if ids.is_empty() {
                    continue;
                }
                constants.insert(format!("{}.{}", t.name, m.name), ids.clone());
                bare.entry(m.name.clone())
                    .and_modify(|v| {
                        if v.as_ref() != Some(&ids) {
                            *v = None;
                        }
                    })
                    .or_insert_with(|| Some(ids));
            }
        }
    }
    // A first literal many constants share is a namespace
    // (`fromNamespaceAndPath("farmersdelight", "stove")`), not an id.
    let mut firsts: HashMap<&str, usize> = HashMap::new();
    for ids in constants.values().filter(|v| v.len() > 1) {
        *firsts.entry(ids[0].as_str()).or_default() += 1;
    }
    let namespaces: BTreeSet<String> = firsts
        .into_iter()
        .filter(|(_, n)| *n >= 3)
        .map(|(ns, _)| ns.to_string())
        .collect();
    let constant_id = |arg: &str, registry: &str| -> Option<String> {
        let a = arg.trim();
        if a.is_empty()
            || !a
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            return None;
        }
        let parts: Vec<&str> = a.split('.').collect();
        let key = parts[parts.len().saturating_sub(2)..].join(".");
        let ids = constants
            .get(&key)
            .cloned()
            .or_else(|| bare.get(*parts.last()?).cloned().flatten())?;
        let ids: Vec<&String> = ids.iter().filter(|i| !namespaces.contains(*i)).collect();
        let pick = if registry == "block" {
            ids.first()
        } else {
            ids.last()
        };
        pick.map(|i| (*i).clone())
    };
    let mut inferred = Vec::new();
    for (_, file) in parsed {
        for call in &file.calls {
            let literal = call
                .literal_args
                .iter()
                .flatten()
                .find(|a| valid_id(a))
                .cloned();
            let has_constant = call.args.iter().any(|a| constant_id(a, "").is_some());
            if literal.is_none() && !has_constant {
                continue;
            }
            // An unqualified call to a method of the mod's own, or a library
            // registrar's `register...` (Balm's `blocks.register("warp_plate", ...)`).
            let own = call.object.is_empty() && file.methods.iter().any(|m| m.name == call.method);
            let registrar = !call.object.is_empty() && call.method.starts_with("reg");
            if !own && !registrar {
                continue;
            }
            let Some(creates) = created_class(&call.args) else {
                continue;
            };
            // Through a library's registrar, only the mod's own classes: a call that
            // names a vanilla class (`register("shulker_box", ShulkerBoxBlockEntity::new)`)
            // is about it, not a registration of the mod's.
            let simple = creates.rsplit('.').next().unwrap_or(&creates);
            if !own
                && !supers.contains_key(simple)
                && !parsed
                    .iter()
                    .any(|(_, f)| f.types.iter().any(|t| t.name == simple))
            {
                continue;
            }
            // A mob whose classes go through a library's: its `MobCategory` says it.
            let mob = call.args.iter().any(|a| a.contains("MobCategory."));
            let Some(registry) = kind_of(&creates).or(mob.then_some("entity_type")) else {
                continue;
            };
            // The id: its first literal that reads as one, wherever it is
            // (`register(func, "blood", p -> new BloodBlock(...), props)`), else a
            // constant holding one (`regBlock(ModBlockItemIds.CABBAGE_CRATE, ...)`).
            let Some(id) =
                literal.or_else(|| call.args.iter().find_map(|a| constant_id(a, registry)))
            else {
                continue;
            };
            let known = out.iter().chain(inferred.iter()).any(|r: &Registration| {
                r.registry == registry && r.id.as_deref() == Some(id.as_str())
            });
            if known {
                continue;
            }
            inferred.push(Registration {
                registry: registry.into(),
                id: Some(id),
                via: "helper method".into(),
                class: qualify(&file.package, &call.enclosing),
                creates: Some(creates),
                args: call.args.join(", "),
            });
        }
    }
    out.extend(inferred);
    // A helper's call with its id in a constant (`regBlock(ModBlockItemIds.STOVE,
    // StoveBlock::new, ...)`): the constant's id.
    for r in out
        .iter_mut()
        .filter(|r| r.id.is_none() && !r.args.is_empty())
    {
        r.id = machines::split_top(&r.args)
            .iter()
            .find_map(|a| constant_id(a, &r.registry));
    }
    // The same id twice in a registry (found two ways): the first.
    let mut seen_ids: BTreeSet<(String, String)> = BTreeSet::new();
    out.retain(|r| match &r.id {
        Some(id) => seen_ids.insert((r.registry.clone(), id.clone())),
        None => true,
    });

    // A helper nobody calls with an id still registers something.
    let mut unused: Vec<_> = helpers
        .iter()
        .filter(|(key, _)| !helper_used.contains(*key))
        .collect();
    unused.sort_by(|a, b| a.0.cmp(b.0));
    for ((class, _), helper) in unused {
        let package = parsed
            .iter()
            .find(|(_, f)| f.types.iter().any(|t| &t.name == class))
            .map(|(_, f)| f.package.as_str())
            .unwrap_or("");
        out.push(Registration {
            registry: helper.registry.clone(),
            id: None,
            via: "helper method".into(),
            class: qualify(package, class),
            creates: None,
            args: String::new(),
        });
    }
    out
}

/// A registry path as ids have them: `inventory_connector`, `block/shaft`.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-./".contains(c))
}

/// The class a registration creates, from its arguments: `CrankBlock` in
/// `CrankBlock::new` or `() -> new CrankBlock(props)`. The first one wins, so
/// `BlockEntityType.Builder.of(CrankBlockEntity::new, CRANK.get())` gives the
/// block entity.
fn created_class(args: &[String]) -> Option<String> {
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    let text = args.join(", ");
    let mut best: Option<(usize, String)> = None;
    if let Some(at) = text.find("::new") {
        let start = text[..at]
            .rfind(|c: char| !is_ident(c) && c != '.')
            .map_or(0, |p| p + 1);
        let name = text[start..at].rsplit('.').next().unwrap_or("").to_string();
        best = Some((at, name));
    }
    let mut from = 0;
    while let Some(i) = text[from..].find("new ").map(|i| from + i) {
        let word_start = i == 0 || !is_ident(text[..i].chars().last().unwrap_or(' '));
        let after = &text[i + 4..];
        let name: String = after
            .chars()
            .take_while(|c| is_ident(*c) || *c == '.')
            .collect();
        let simple = name.rsplit('.').next().unwrap_or("").to_string();
        if word_start && simple.starts_with(|c: char| c.is_ascii_uppercase()) {
            if best.as_ref().is_none_or(|(at, _)| i < *at) {
                best = Some((i, simple));
            }
            break;
        }
        from = i + 4;
    }
    best.map(|(_, name)| name)
        .filter(|n| n.starts_with(|c: char| c.is_ascii_uppercase()))
}

/// The values a class writes to NBT: `tag.putFloat("Speed", speed)`.
fn saved_values(file: &ParsedFile, class: &str) -> Vec<porter_model::SavedValue> {
    let mut out = Vec::new();
    for call in file.calls.iter().filter(|c| c.enclosing == class) {
        if !call.method.starts_with("put") || call.args.len() != 2 {
            continue;
        }
        let Some(key) = call.literal_args.first().cloned().flatten() else {
            continue;
        };
        out.push(porter_model::SavedValue {
            key,
            method: call.method.clone(),
            value: call.args[1].trim().to_string(),
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_second_type_argument() {
        assert_eq!(
            second_type_argument("BiConsumer<Block, ResourceLocation>"),
            Some("ResourceLocation")
        );
        assert_eq!(
            second_type_argument(
                "BiConsumer<Map<String, Block>, net.minecraft.resources.Identifier>"
            ),
            Some("net.minecraft.resources.Identifier")
        );
        assert_eq!(second_type_argument("Consumer<Block>"), None);
    }

    #[test]
    fn finds_id_parameters() {
        let params = vec!["english".to_string(), "id".to_string()];
        assert_eq!(id_param("id.getPath()", &params), Some(1));
        assert_eq!(id_param("prefix(id)", &params), Some(1));
        assert_eq!(id_param("\"id\"", &params), None);
        assert_eq!(id_param("\"a \\\" id\" + other", &params), None);
    }
}
