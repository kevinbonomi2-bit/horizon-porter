// Parses one Java source file with tree-sitter into the facts the scanner needs:
// package, imports, top-level types with their annotations and members, registry
// calls and DeferredRegister fields. Names are left as written; lib.rs resolves them.

use anyhow::{Result, anyhow};
use porter_model::ClassKind;
use tree_sitter::{Node, Parser};

pub(crate) fn new_parser() -> Result<Parser> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .map_err(|e| anyhow!("loading the Java grammar: {e}"))?;
    Ok(parser)
}

pub(crate) struct Import {
    /// `net.minecraft.world.level.Level`, or the package for a wildcard.
    pub path: String,
    pub is_static: bool,
    pub wildcard: bool,
}

/// An annotation argument value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Str(String),
    /// `Foo.class`: the type as written.
    Class(String),
    /// A name or field access, such as `At.Shift.AFTER` or `Dist.CLIENT`.
    Name(String),
    Bool(bool),
    Annotation(Annotation),
    Array(Vec<Value>),
    Other(String),
}

impl Value {
    /// The value, or each element of an array.
    pub fn items(&self) -> Vec<&Value> {
        match self {
            Value::Array(items) => items.iter().collect(),
            other => vec![other],
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Text of a name, a string or other value, for loose matching.
    pub fn text(&self) -> String {
        match self {
            Value::Str(s) | Value::Class(s) | Value::Name(s) | Value::Other(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Annotation(a) => format!("@{}", a.name),
            Value::Array(items) => items.iter().map(Value::text).collect::<Vec<_>>().join(", "),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Annotation {
    /// As written: `Mixin` or `org.spongepowered.asm.mixin.Mixin`.
    pub name: String,
    /// Named arguments; a single unnamed argument is stored as `value`.
    pub args: Vec<(String, Value)>,
}

impl Annotation {
    pub fn simple_name(&self) -> &str {
        self.name.rsplit('.').next().unwrap_or(&self.name)
    }

    pub fn arg(&self, key: &str) -> Option<&Value> {
        self.args.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

pub(crate) struct Member {
    pub name: String,
    pub is_method: bool,
    pub is_constructor: bool,
    pub annotations: Vec<Annotation>,
}

pub(crate) struct TypeDecl {
    pub name: String,
    pub kind: ClassKind,
    pub superclass: Option<String>,
    pub interfaces: Vec<String>,
    pub annotations: Vec<Annotation>,
    pub members: Vec<Member>,
    pub methods: usize,
}

/// A method call that might register something, directly or through a helper
/// method: registry calls, and calls passing a string literal or one of the
/// enclosing method's parameters.
pub(crate) struct Call {
    /// The receiver as written: `BLOCKS`, `ModBlocks.BLOCKS`, `Registry`, `REGISTRATE`;
    /// empty for an unqualified call.
    pub object: String,
    /// Whether the receiver is itself a call (a builder chain), other than
    /// `registrate()`.
    pub object_is_chain: bool,
    pub method: String,
    /// The text of each argument.
    pub args: Vec<String>,
    /// Each argument's value when it is a string literal.
    pub literal_args: Vec<Option<String>>,
    /// The first argument, when it is a string literal.
    pub first_string: Option<String>,
    /// The last string literal inside the second argument (Fabric ids).
    pub second_arg_last_string: Option<String>,
    /// The top-level type it is in.
    pub enclosing: String,
    /// The method it is in, as an index into [`ParsedFile::methods`].
    pub in_method: Option<usize>,
}

/// A method's name, parameter names and parameter types (as written).
pub(crate) struct MethodSig {
    pub name: String,
    pub params: Vec<String>,
    pub param_types: Vec<String>,
}

/// A field holding a DeferredRegister, with the registry it registers into.
pub(crate) struct RegisterField {
    /// The simple name of the top-level type that declares it.
    pub class: String,
    pub field: String,
    pub registry: String,
}

pub(crate) struct ParsedFile {
    pub package: String,
    pub imports: Vec<Import>,
    pub types: Vec<TypeDecl>,
    pub lines: usize,
    pub calls: Vec<Call>,
    pub registers: Vec<RegisterField>,
    /// Fully qualified type names used in code, such as `net.minecraft.world.level.Level`.
    pub qualified_refs: Vec<String>,
    /// Methods and constructors, referred to by [`Call::in_method`].
    pub methods: Vec<MethodSig>,
    /// The first line (1-based) tree-sitter could not parse, if any.
    pub syntax_error_line: Option<usize>,
}

/// Method names that register through a DeferredRegister or `Registry.register`.
const REGISTER_METHODS: &[&str] = &[
    "register",
    "registerBlock",
    "registerSimpleBlock",
    "registerItem",
    "registerSimpleItem",
    "registerForHolder",
    "registerReference",
];

/// Registrate builder entry points and the registry each one fills.
pub(crate) const REGISTRATE_METHODS: &[(&str, &str)] = &[
    ("block", "block"),
    ("item", "item"),
    ("blockEntity", "block_entity_type"),
    ("entity", "entity_type"),
    ("fluid", "fluid"),
    ("standardFluid", "fluid"),
    ("virtualFluid", "fluid"),
    ("menu", "menu"),
    ("enchantment", "enchantment"),
    ("effect", "mob_effect"),
    ("recipeSerializer", "recipe_serializer"),
];

pub(crate) fn parse(parser: &mut Parser, src: &str) -> Result<ParsedFile> {
    let tree = parser
        .parse(src, None)
        .ok_or_else(|| anyhow!("tree-sitter returned no tree"))?;
    let root = tree.root_node();
    let text = |n: Node| n.utf8_text(src.as_bytes()).unwrap_or("").to_string();

    let mut file = ParsedFile {
        package: String::new(),
        imports: Vec::new(),
        types: Vec::new(),
        lines: src.lines().count(),
        calls: Vec::new(),
        registers: Vec::new(),
        qualified_refs: Vec::new(),
        methods: Vec::new(),
        syntax_error_line: root.has_error().then(|| first_error_line(root)),
    };

    for node in named_children(root) {
        match node.kind() {
            "package_declaration" => {
                if let Some(name) = named_children(node)
                    .into_iter()
                    .find(|c| matches!(c.kind(), "scoped_identifier" | "identifier"))
                {
                    file.package = text(name);
                }
            }
            "import_declaration" => file.imports.push(parse_import(&text(node))),
            kind if type_kind(kind).is_some() => {
                let mut decl = parse_type(node, src);
                decl.methods = walk_type(node, src, &decl.name, &mut file);
                file.types.push(decl);
            }
            _ => {}
        }
    }
    Ok(file)
}

/// The line of the first node tree-sitter marked as an error or as missing.
fn first_error_line(root: Node) -> usize {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() {
            return node.start_position().row + 1;
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        stack.extend(
            children
                .into_iter()
                .rev()
                .filter(|c| c.has_error() || c.is_missing()),
        );
    }
    root.start_position().row + 1
}

/// One pass over a top-level type: collects its registry calls, DeferredRegister
/// fields and fully qualified type names into `file`, and returns its method count.
fn walk_type(type_node: Node, src: &str, enclosing: &str, file: &mut ParsedFile) -> usize {
    let text = |n: Node| n.utf8_text(src.as_bytes()).unwrap_or("").to_string();
    let mut methods = 0;
    // (node, kind of its parent, the method it is in)
    let mut stack: Vec<(Node, &str, Option<usize>)> = vec![(type_node, "", None)];
    while let Some((node, parent_kind, mut in_method)) = stack.pop() {
        match node.kind() {
            "method_declaration" | "constructor_declaration" => {
                methods += 1;
                let (params, param_types) = node
                    .child_by_field_name("parameters")
                    .map(|p| {
                        named_children(p)
                            .into_iter()
                            .filter_map(|param| {
                                let name = param.child_by_field_name("name").map(text)?;
                                let ty = param.child_by_field_name("type").map(text);
                                Some((name, ty.unwrap_or_default()))
                            })
                            .unzip()
                    })
                    .unwrap_or_default();
                file.methods.push(MethodSig {
                    name: node
                        .child_by_field_name("name")
                        .map(text)
                        .unwrap_or_default(),
                    params,
                    param_types,
                });
                in_method = Some(file.methods.len() - 1);
            }
            "method_invocation" => {
                let params = in_method.map_or(&[][..], |i| &file.methods[i].params[..]);
                file.calls
                    .extend(call_at(node, src, enclosing, params, in_method));
            }
            "field_declaration" => register_fields_at(node, src, enclosing, &mut file.registers),
            // `a.b.C` nests a scoped_type_identifier for each dot: only the
            // outermost is the whole name.
            "scoped_type_identifier" if parent_kind != "scoped_type_identifier" => {
                let t = base_type(node.utf8_text(src.as_bytes()).unwrap_or(""));
                if t.contains('.') && t.starts_with(|c: char| c.is_ascii_lowercase()) {
                    file.qualified_refs.push(t);
                }
            }
            _ => {}
        }
        // Pushed in reverse so they pop in source order.
        let kind = node.kind();
        stack.extend(
            named_children(node)
                .into_iter()
                .rev()
                .map(|c| (c, kind, in_method)),
        );
    }
    methods
}

fn type_kind(kind: &str) -> Option<ClassKind> {
    Some(match kind {
        "class_declaration" => ClassKind::Class,
        "interface_declaration" => ClassKind::Interface,
        "enum_declaration" => ClassKind::Enum,
        "record_declaration" => ClassKind::Record,
        "annotation_type_declaration" => ClassKind::Annotation,
        _ => return None,
    })
}

fn parse_import(text: &str) -> Import {
    let body: String = text
        .trim()
        .trim_start_matches("import")
        .trim_end_matches(';')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let (is_static, body) = match body.strip_prefix("static ") {
        Some(rest) => (true, rest.to_string()),
        None => (false, body),
    };
    let path: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    match path.strip_suffix(".*") {
        Some(pkg) => Import {
            path: pkg.to_string(),
            is_static,
            wildcard: true,
        },
        None => Import {
            path,
            is_static,
            wildcard: false,
        },
    }
}

fn parse_type(node: Node, src: &str) -> TypeDecl {
    let text = |n: Node| n.utf8_text(src.as_bytes()).unwrap_or("").to_string();
    let mut decl = TypeDecl {
        name: node
            .child_by_field_name("name")
            .map(text)
            .unwrap_or_default(),
        kind: type_kind(node.kind()).unwrap_or(ClassKind::Class),
        superclass: None,
        interfaces: Vec::new(),
        annotations: Vec::new(),
        members: Vec::new(),
        methods: 0, // counted by walk_type
    };
    for child in named_children(node) {
        match child.kind() {
            "modifiers" => decl.annotations = annotations_of(child, src),
            "superclass" => {
                decl.superclass = named_children(child).first().map(|t| base_type(&text(*t)));
            }
            "super_interfaces" | "extends_interfaces" => {
                for list in named_children(child) {
                    for t in named_children(list) {
                        decl.interfaces.push(base_type(&text(t)));
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(body) = node.child_by_field_name("body") {
        collect_members(body, src, &mut decl.members);
    }
    decl
}

fn collect_members(body: Node, src: &str, out: &mut Vec<Member>) {
    let text = |n: Node| n.utf8_text(src.as_bytes()).unwrap_or("").to_string();
    for child in named_children(body) {
        match child.kind() {
            "enum_body_declarations" => collect_members(child, src, out),
            "method_declaration" | "constructor_declaration" => {
                let annotations = named_children(child)
                    .into_iter()
                    .find(|c| c.kind() == "modifiers")
                    .map(|m| annotations_of(m, src))
                    .unwrap_or_default();
                out.push(Member {
                    name: child
                        .child_by_field_name("name")
                        .map(text)
                        .unwrap_or_default(),
                    is_method: true,
                    is_constructor: child.kind() == "constructor_declaration",
                    annotations,
                });
            }
            "field_declaration" => {
                let annotations = named_children(child)
                    .into_iter()
                    .find(|c| c.kind() == "modifiers")
                    .map(|m| annotations_of(m, src))
                    .unwrap_or_default();
                for declarator in named_children(child)
                    .into_iter()
                    .filter(|c| c.kind() == "variable_declarator")
                {
                    out.push(Member {
                        name: declarator
                            .child_by_field_name("name")
                            .map(text)
                            .unwrap_or_default(),
                        is_method: false,
                        is_constructor: false,
                        annotations: annotations.clone(),
                    });
                }
            }
            _ => {}
        }
    }
}

fn annotations_of(modifiers: Node, src: &str) -> Vec<Annotation> {
    named_children(modifiers)
        .into_iter()
        .filter(|c| matches!(c.kind(), "annotation" | "marker_annotation"))
        .map(|a| parse_annotation(a, src))
        .collect()
}

fn parse_annotation(node: Node, src: &str) -> Annotation {
    let text = |n: Node| n.utf8_text(src.as_bytes()).unwrap_or("").to_string();
    let mut annotation = Annotation {
        name: node
            .child_by_field_name("name")
            .map(text)
            .unwrap_or_default(),
        args: Vec::new(),
    };
    if let Some(list) = node.child_by_field_name("arguments") {
        for arg in named_children(list).into_iter().filter(|n| !is_comment(*n)) {
            if arg.kind() == "element_value_pair" {
                let key = arg.child_by_field_name("key").map(text).unwrap_or_default();
                if let Some(value) = arg.child_by_field_name("value") {
                    annotation.args.push((key, parse_value(value, src)));
                }
            } else {
                annotation
                    .args
                    .push(("value".to_string(), parse_value(arg, src)));
            }
        }
    }
    annotation
}

fn parse_value(node: Node, src: &str) -> Value {
    let text = node.utf8_text(src.as_bytes()).unwrap_or("").to_string();
    match node.kind() {
        "string_literal" => Value::Str(unquote(&text)),
        "class_literal" => Value::Class(
            named_children(node)
                .first()
                .map(|t| base_type(t.utf8_text(src.as_bytes()).unwrap_or("")))
                .unwrap_or_default(),
        ),
        "identifier" | "field_access" | "scoped_identifier" => Value::Name(text),
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "annotation" | "marker_annotation" => Value::Annotation(parse_annotation(node, src)),
        "element_value_array_initializer" => Value::Array(
            named_children(node)
                .into_iter()
                .filter(|n| !is_comment(*n))
                .map(|n| parse_value(n, src))
                .collect(),
        ),
        "parenthesized_expression" => named_children(node)
            .first()
            .map(|n| parse_value(*n, src))
            .unwrap_or(Value::Other(text)),
        // "a" + "b": constant string concatenation.
        "binary_expression" => {
            let parts: Vec<Node> = descendants(node)
                .into_iter()
                .filter(|n| n.kind() == "string_literal")
                .collect();
            let only_strings = descendants(node).iter().all(|n| {
                matches!(
                    n.kind(),
                    "binary_expression" | "string_literal" | "string_fragment" | "escape_sequence"
                )
            });
            if only_strings && !parts.is_empty() {
                Value::Str(
                    parts
                        .iter()
                        .map(|p| unquote(p.utf8_text(src.as_bytes()).unwrap_or("")))
                        .collect(),
                )
            } else {
                Value::Other(text)
            }
        }
        _ => Value::Other(text),
    }
}

/// The call at `node` if it might register something, directly or through a helper.
/// `params` are the enclosing method's parameter names.
fn call_at(
    node: Node,
    src: &str,
    enclosing: &str,
    params: &[String],
    in_method: Option<usize>,
) -> Option<Call> {
    let text = |n: Node| n.utf8_text(src.as_bytes()).unwrap_or("").to_string();
    let method = node
        .child_by_field_name("name")
        .map(text)
        .unwrap_or_default();
    let object_node = node.child_by_field_name("object");
    let object = object_node.map(text).unwrap_or_default();
    let args: Vec<Node> = node
        .child_by_field_name("arguments")
        .map(|a| {
            named_children(a)
                .into_iter()
                .filter(|n| !is_comment(*n))
                .collect()
        })
        .unwrap_or_default();
    let literal_args: Vec<Option<String>> = args
        .iter()
        .map(|a| (a.kind() == "string_literal").then(|| unquote(&text(*a))))
        .collect();

    let is_registrate = REGISTRATE_METHODS.iter().any(|(m, _)| *m == method)
        && object.to_ascii_lowercase().contains("registrate");
    // `r.accept(block, id)` on a callback parameter (multi-loader registration).
    let is_callback = method == "accept" && params.contains(&object);
    let is_register = REGISTER_METHODS.contains(&method.as_str()) || is_registrate || is_callback;
    // Possible helper call sites: a string literal, or a parameter passed on.
    let maybe_helper = literal_args.iter().any(Option::is_some)
        || args
            .iter()
            .any(|a| a.kind() == "identifier" && params.iter().any(|p| *p == text(*a)));
    if !is_register && !maybe_helper {
        return None;
    }
    let object_is_chain = object_node.is_some_and(|o| {
        o.kind() == "method_invocation"
            && o.child_by_field_name("name").map(text).as_deref() != Some("registrate")
    });
    let first_string = literal_args.first().cloned().flatten();
    let second_arg_last_string = args.get(1).and_then(|a| {
        descendants(*a)
            .into_iter()
            .rfind(|n| n.kind() == "string_literal")
            .map(|n| unquote(&text(n)))
    });
    Some(Call {
        object,
        object_is_chain,
        method,
        args: args.iter().map(|a| text(*a)).collect(),
        literal_args,
        first_string,
        second_arg_last_string,
        enclosing: enclosing.to_string(),
        in_method,
    })
}

/// DeferredRegister fields declared by the field declaration at `field`.
fn register_fields_at(field: Node, src: &str, enclosing: &str, out: &mut Vec<RegisterField>) {
    let text = |n: Node| n.utf8_text(src.as_bytes()).unwrap_or("").to_string();
    let ty = field
        .child_by_field_name("type")
        .map(text)
        .unwrap_or_default();
    if !ty.contains("DeferredRegister") {
        return;
    }
    for declarator in named_children(field)
        .into_iter()
        .filter(|c| c.kind() == "variable_declarator")
    {
        let name = declarator
            .child_by_field_name("name")
            .map(text)
            .unwrap_or_default();
        let init = declarator
            .child_by_field_name("value")
            .map(text)
            .unwrap_or_default();
        if let Some(registry) = deferred_register_registry(&ty, &init) {
            out.push(RegisterField {
                class: enclosing.to_string(),
                field: name,
                registry,
            });
        }
    }
}

/// The registry a DeferredRegister field fills, from its initializer or its type.
pub(crate) fn deferred_register_registry(ty: &str, init: &str) -> Option<String> {
    // Registries.BLOCK, BuiltInRegistries.ITEM, ForgeRegistries.BLOCK_ENTITY_TYPES,
    // NeoForgeRegistries.Keys.ENTITY_DATA_SERIALIZERS.
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut from = 0;
    while let Some(i) = init[from..].find("Registries.").map(|i| from + i) {
        let owner_start = init[..i].rfind(|c: char| !is_ident(c)).map_or(0, |p| p + 1);
        let owner = &init[owner_start..i + "Registries".len()];
        let after = &init[i + "Registries.".len()..];
        let after = after.strip_prefix("Keys.").unwrap_or(after);
        let name: String = after.chars().take_while(|c| is_ident(*c)).collect();
        if !name.is_empty() {
            let lower = name.to_ascii_lowercase();
            // Vanilla's names are singular; Forge's and NeoForge's are plural.
            let registry = match owner {
                "Registries" | "BuiltInRegistries" => lower.as_str(),
                _ => lower.strip_suffix('s').unwrap_or(&lower),
            };
            return Some(normalize_registry(registry));
        }
        from = i + 1;
    }
    for (needle, registry) in [
        ("createBlocks", "block"),
        ("createItems", "item"),
        ("createDataComponents", "data_component_type"),
        ("DeferredRegister.Blocks", "block"),
        ("DeferredRegister.Items", "item"),
        ("DeferredRegister.DataComponents", "data_component_type"),
    ] {
        if init.contains(needle) || ty.contains(needle) {
            return Some(registry.to_string());
        }
    }
    // DeferredRegister<BlockEntityType<?>> → block_entity_type
    first_type_argument_registry(ty)
}

/// The registry named by a type's first type argument: `DeferredRegister<Block>` and
/// `BiConsumer<BlockEntityType<?>, ResourceLocation>` give `block` and
/// `block_entity_type`.
pub(crate) fn first_type_argument_registry(ty: &str) -> Option<String> {
    let generic = ty.split_once('<')?.1;
    let base: String = generic
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        .collect();
    let base = base.rsplit('.').next()?;
    if base.is_empty() || base == "T" || base == "?" {
        return None;
    }
    Some(normalize_registry(&snake_case(base)))
}

fn normalize_registry(name: &str) -> String {
    match name {
        "menu_type" => "menu",
        "potion_type" => "potion",
        "enchantment_type" => "enchantment",
        other => other,
    }
    .to_string()
}

fn snake_case(camel: &str) -> String {
    let mut out = String::new();
    for (i, c) in camel.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// `List<Foo>` → `List`; `Foo[]` → `Foo`; `@Nullable Foo` → `Foo`.
pub(crate) fn base_type(text: &str) -> String {
    let t = text.split('<').next().unwrap_or(text);
    let t = t.trim().trim_end_matches("[]").trim();
    let t = t.rsplit(' ').next().unwrap_or(t);
    t.to_string()
}

fn unquote(text: &str) -> String {
    let t = text.trim();
    if let Some(inner) = t
        .strip_prefix("\"\"\"")
        .and_then(|s| s.strip_suffix("\"\"\""))
    {
        return inner.trim().to_string();
    }
    t.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(t)
        .to_string()
}

fn is_comment(node: Node) -> bool {
    matches!(node.kind(), "line_comment" | "block_comment")
}

fn named_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

/// Every named node under `node` (including itself), depth first, without recursion.
fn descendants(node: Node) -> Vec<Node> {
    let mut out = Vec::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        out.push(n);
        let mut children = named_children(n);
        children.reverse();
        stack.extend(children);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_src(src: &str) -> ParsedFile {
        parse(&mut new_parser().unwrap(), src).unwrap()
    }

    #[test]
    fn reads_package_imports_and_types() {
        let f = parse_src(
            r#"
package com.demo.block;

import net.minecraft.world.level.block.Block;
import static net.minecraft.world.level.block.Blocks.STONE;
import net.minecraft.world.item.*;

public class ShaftBlock extends RotatedBlock<Foo> implements IWrenchable, java.io.Serializable {
    public ShaftBlock(Properties p) { super(p); }
    void tick() { net.minecraft.world.level.Level level = null; }
}
"#,
        );
        assert_eq!(f.package, "com.demo.block");
        assert_eq!(f.imports.len(), 3);
        assert!(f.imports[1].is_static);
        assert!(f.imports[2].wildcard);
        assert_eq!(f.imports[2].path, "net.minecraft.world.item");
        let t = &f.types[0];
        assert_eq!(t.name, "ShaftBlock");
        assert_eq!(t.superclass.as_deref(), Some("RotatedBlock"));
        assert_eq!(t.interfaces, vec!["IWrenchable", "java.io.Serializable"]);
        assert_eq!(t.methods, 2);
        assert_eq!(
            f.qualified_refs,
            vec!["java.io.Serializable", "net.minecraft.world.level.Level"]
        );
        assert_eq!(f.syntax_error_line, None);
    }

    #[test]
    fn reads_mixin_annotations() {
        let f = parse_src(
            r#"
@Mixin(value = {Level.class, ServerLevel.class}, priority = 900)
public abstract class LevelMixin implements Extra {
    @Shadow @Final private int x;
    @Inject(method = {"tick", "tickBlockEntities"}, at = @At(value = "INVOKE",
        target = "Lnet/minecraft/world/level/Level;foo()V", shift = At.Shift.AFTER), cancellable = true)
    private void demo$tick(CallbackInfo ci) {}
    @Redirect(method = "a" + "b", at = @At("HEAD"))
    private void r() {}
}
"#,
        );
        let t = &f.types[0];
        let mixin = &t.annotations[0];
        assert_eq!(mixin.simple_name(), "Mixin");
        assert_eq!(
            mixin.arg("value"),
            Some(&Value::Array(vec![
                Value::Class("Level".into()),
                Value::Class("ServerLevel".into())
            ]))
        );
        assert_eq!(t.members.len(), 3);
        assert_eq!(t.members[0].name, "x");
        assert_eq!(t.members[0].annotations[0].simple_name(), "Shadow");
        let inject = &t.members[1].annotations[0];
        assert_eq!(inject.arg("method").unwrap().items().len(), 2);
        assert_eq!(inject.arg("cancellable"), Some(&Value::Bool(true)));
        let Some(Value::Annotation(at)) = inject.arg("at") else {
            panic!("no @At");
        };
        assert_eq!(at.arg("value"), Some(&Value::Str("INVOKE".into())));
        assert_eq!(at.arg("shift"), Some(&Value::Name("At.Shift.AFTER".into())));
        let redirect = &t.members[2].annotations[0];
        assert_eq!(redirect.arg("method"), Some(&Value::Str("ab".into())));
    }

    #[test]
    fn finds_registrations() {
        let f = parse_src(
            r#"
public class ModBlocks {
    public static final DeferredRegister.Blocks BLOCKS = DeferredRegister.createBlocks(ID);
    static final DeferredRegister<BlockEntityType<?>> BES = DeferredRegister.create(Registries.BLOCK_ENTITY_TYPE, ID);
    public static final DeferredBlock<Block> RUBY = BLOCKS.registerSimpleBlock("ruby", props());
    static final BlockEntry<ShaftBlock> SHAFT = REGISTRATE.block("shaft", ShaftBlock::new).item().build().register();
    static Item GEM = Registry.register(BuiltInRegistries.ITEM, Identifier.of("demo", "gem"), new Item(p));
}
"#,
        );
        assert_eq!(f.registers.len(), 2);
        assert_eq!(f.registers[0].registry, "block");
        assert_eq!(f.registers[1].registry, "block_entity_type");
        assert_eq!(
            deferred_register_registry(
                "DeferredRegister<EntityDataSerializer<?>>",
                "DeferredRegister.create(NeoForgeRegistries.Keys.ENTITY_DATA_SERIALIZERS, ID)"
            )
            .as_deref(),
            Some("entity_data_serializer")
        );
        assert_eq!(
            deferred_register_registry(
                "",
                "DeferredRegister.create(ForgeRegistries.MENU_TYPES, ID)"
            )
            .as_deref(),
            Some("menu")
        );
        let methods: Vec<_> = f.calls.iter().map(|c| c.method.as_str()).collect();
        assert!(methods.contains(&"registerSimpleBlock"));
        assert!(methods.contains(&"block"));
        let fabric = f.calls.iter().find(|c| c.object == "Registry").unwrap();
        assert_eq!(fabric.second_arg_last_string.as_deref(), Some("gem"));
        // The chained .item() is a builder step, not a registration.
        assert!(
            f.calls
                .iter()
                .all(|c| c.method != "item" || c.object_is_chain)
        );
    }
}
