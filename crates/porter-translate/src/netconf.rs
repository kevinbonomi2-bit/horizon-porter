//! Network messages and config, ported.
//!
//! Each `CustomPacketPayload` record becomes a Rust struct with its channel and an
//! encoding (the port's own: both ends are this port), and a handler to port for
//! each way it travels. `native_net_from_client` and `native_net_from_server` decode
//! what arrives and call them; what a handler sends goes out through an `Outbox`.
//!
//! The config becomes a `Config` struct whose values are kept in Horizon's mod
//! settings, one per key, and the mod's page in Options > Mods: a button per value
//! that toggles it, cycles it, or steps it through its range.

use std::fmt::Write;

use porter_model::{ConfigEntry, ConfigKind, PayloadInfo};

/// A payload, ready to generate.
#[derive(Debug, Clone)]
pub struct Message {
    /// The Rust struct: `SetSpeed`.
    pub name: String,
    /// `set_speed`, for its handlers.
    pub snake: String,
    pub channel: String,
    /// (Rust field, Rust type, Java type).
    pub fields: Vec<(String, Type, String)>,
    pub to_server: bool,
    pub to_client: bool,
    pub class: String,
    /// The classes that register it, where its handlers are.
    pub registered_in: Vec<String>,
}

/// The Rust type a Java payload field becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    I32,
    I64,
    F32,
    F64,
    Bool,
    Text,
    Pos,
    Vec3,
    Uuid,
    /// A type the porter can't encode: its bytes, to fill in.
    Bytes,
}

impl Type {
    fn of(java: &str) -> Type {
        let base = java.rsplit('.').next().unwrap_or(java);
        match base {
            "int" | "Integer" | "short" | "Short" | "byte" | "Byte" | "char" | "Character" => {
                Type::I32
            }
            "long" | "Long" => Type::I64,
            "float" | "Float" => Type::F32,
            "double" | "Double" => Type::F64,
            "boolean" | "Boolean" => Type::Bool,
            "String" | "ResourceLocation" | "Identifier" => Type::Text,
            "BlockPos" => Type::Pos,
            "Vec3" | "Vec3d" => Type::Vec3,
            "UUID" => Type::Uuid,
            _ => Type::Bytes,
        }
    }

    fn rust(self) -> &'static str {
        match self {
            Type::I32 => "i32",
            Type::I64 => "i64",
            Type::F32 => "f32",
            Type::F64 => "f64",
            Type::Bool => "bool",
            Type::Text => "String",
            Type::Pos => "[i32; 3]",
            Type::Vec3 => "[f64; 3]",
            Type::Uuid => "u128",
            Type::Bytes => "Vec<u8>",
        }
    }

    /// The `Writer`/`Reader` method for it.
    fn method(self) -> &'static str {
        match self {
            Type::I32 => "i32",
            Type::I64 => "i64",
            Type::F32 => "f32",
            Type::F64 => "f64",
            Type::Bool => "bool",
            Type::Text => "text",
            Type::Pos => "pos",
            Type::Vec3 => "vec3",
            Type::Uuid => "uuid",
            Type::Bytes => "bytes",
        }
    }

    fn sample(self) -> &'static str {
        match self {
            Type::I32 => "-7",
            Type::I64 => "1 << 40",
            Type::F32 => "1.5",
            Type::F64 => "-2.25",
            Type::Bool => "true",
            Type::Text => "\"héllo\".to_string()",
            Type::Pos => "[1, -64, 300]",
            Type::Vec3 => "[0.5, 64.0, -3.25]",
            Type::Uuid => "0x0123_4567_89ab_cdef_0123_4567_89ab_cdef",
            Type::Bytes => "vec![1, 2, 3]",
        }
    }
}

/// The types a port's runtime defines itself, which a message can't be named.
const RESERVED: &[&str] = &[
    "Payload",
    "Outbox",
    "Writer",
    "Reader",
    "Machine",
    "MachineParts",
    "Stack",
    "Recipe",
    "Tank",
    "Side",
    "Transfer",
    "Neighbour",
    "Sprite",
    "Draw",
    "DrawOp",
    "Frame",
    "Gui",
    "GuiOp",
    "Screen",
    "Config",
    "Saved",
    "MachineMenu",
];

/// The messages to generate, from the scan's payloads.
pub fn messages(payloads: &[PayloadInfo], mod_id: &str) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    for p in payloads {
        let mut name = p.class.rsplit('.').next().unwrap_or(&p.class).to_string();
        // Not one of the port's own types (Shulker Box Tooltip has a `Payload`).
        if RESERVED.contains(&name.as_str()) {
            name.push_str("Message");
        }
        if out.iter().any(|m| m.name == name) {
            continue;
        }
        let snake = snake(&name);
        // One message a channel: a second one read the same channel (from a
        // literal that isn't its own), so it takes one named after itself.
        let own = format!("{mod_id}:{snake}");
        let channel = p
            .channel
            .clone()
            .filter(|c| !out.iter().any(|m| m.channel == *c))
            .unwrap_or(own);
        out.push(Message {
            channel,
            fields: p
                .fields
                .iter()
                .map(|f| {
                    (
                        rust_field(&f.name),
                        Type::of(&f.java_type),
                        f.java_type.clone(),
                    )
                })
                .collect(),
            to_server: p.to_server,
            to_client: p.to_client,
            class: p.class.clone(),
            registered_in: p.registered_in.clone(),
            name,
            snake,
        });
    }
    out
}

/// The generated networking: message types, their encoding, handlers to port and
/// the dispatch the hook helpers call.
pub fn net_runtime(messages: &[Message]) -> String {
    if messages.is_empty() {
        return String::new();
    }
    let mut r = String::from(NET_TYPES);
    for m in messages {
        let _ = write!(
            r,
            "\n/// `{class}`, on channel `{channel}`.\n#[derive(Debug, Clone, PartialEq)]\npub struct {name} {{\n",
            class = m.class,
            channel = m.channel,
            name = m.name
        );
        for (field, ty, java) in &m.fields {
            if *ty == Type::Bytes {
                let _ = writeln!(
                    r,
                    "    /// TODO: the Java `{java}`; encode it into these bytes yourself."
                );
            }
            let _ = writeln!(r, "    pub {field}: {},", ty.rust());
        }
        // A message without fields reads and writes nothing.
        let mutable = if m.fields.is_empty() { "" } else { "mut " };
        let _ = write!(
            r,
            "}}\n\nimpl Payload for {name} {{\n    const CHANNEL: &'static str = \"{channel}\";\n\n    fn encode(&self) -> Vec<u8> {{\n        let {mutable}w = Writer::default();\n",
            name = m.name,
            channel = m.channel
        );
        for (field, ty, _) in &m.fields {
            let arg = match ty {
                Type::Text | Type::Bytes => format!("&self.{field}"),
                _ => format!("self.{field}"),
            };
            let _ = writeln!(r, "        w.{}({arg});", ty.method());
        }
        let _ = write!(
            r,
            "        w.0\n    }}\n\n    fn decode(data: &[u8]) -> Option<Self> {{\n        let {mutable}r = Reader(data);\n        let msg = {name} {{\n",
            name = m.name
        );
        for (field, ty, _) in &m.fields {
            let _ = writeln!(r, "            {field}: r.{}()?,", ty.method());
        }
        r.push_str("        };\n        r.0.is_empty().then_some(msg)\n    }\n}\n");
        if m.to_server {
            let _ = write!(
                r,
                "\n/// The server got `{name}` from `player`'s client. Send replies with `out`.\n///\n/// TODO: port the server handler of `{class}` (its `playToServer` or\n/// `registerGlobalReceiver` lambda{place}).\npub fn on_{snake}_from_client(player: u128, msg: {name}, out: &mut Outbox) {{\n    let _ = (player, msg, out);\n}}\n",
                name = m.name,
                class = m.class,
                snake = m.snake,
                place = registered(m)
            );
        }
        if m.to_client {
            let _ = write!(
                r,
                "\n/// This client got `{name}` from the server. Send replies with `out`.\n///\n/// TODO: port the client handler of `{class}` (its `playToClient` or\n/// `registerGlobalReceiver` lambda{place}).\npub fn on_{snake}_from_server(msg: {name}, out: &mut Outbox) {{\n    let _ = (msg, out);\n}}\n",
                name = m.name,
                class = m.class,
                snake = m.snake,
                place = registered(m)
            );
        }
    }
    for (side, to_server) in [("client", true), ("server", false)] {
        let ms: Vec<&Message> = messages
            .iter()
            .filter(|m| if to_server { m.to_server } else { m.to_client })
            .collect();
        if ms.is_empty() {
            continue;
        }
        if to_server {
            r.push_str("\n/// A message from `player`'s client: decodes it and calls its handler.\npub fn net_from_client(player: u128, channel: &str, data: &[u8], out: &mut Outbox) {\n    match channel {\n");
        } else {
            r.push_str("\n/// A message from the server: decodes it and calls its handler.\npub fn net_from_server(channel: &str, data: &[u8], out: &mut Outbox) {\n    match channel {\n");
        }
        for m in ms {
            let call = if to_server {
                format!("on_{}_from_client(player, msg, out)", m.snake)
            } else {
                format!("on_{}_from_server(msg, out)", m.snake)
            };
            let _ = writeln!(
                r,
                "        {name}::CHANNEL => {{\n            if let Some(msg) = {name}::decode(data) {{\n                {call};\n            }}\n        }}",
                name = m.name
            );
        }
        let _ = writeln!(r, "        _ => {{}}\n    }}\n}}\n// (from the {side})");
    }
    r
}

const NET_TYPES: &str = r#"
/// A network message: its channel and its bytes. Both ends are this port, so the
/// encoding is its own (little-endian numbers, length-prefixed text), not Java's.
pub trait Payload: Sized {
    const CHANNEL: &'static str;
    fn encode(&self) -> Vec<u8>;
    fn decode(data: &[u8]) -> Option<Self>;
}

/// Messages a handler sends; the hook helpers send them after it returns.
#[derive(Debug, Default)]
pub struct Outbox {
    pub to_server: Vec<(&'static str, Vec<u8>)>,
    pub to_players: Vec<(u128, &'static str, Vec<u8>)>,
}

impl Outbox {
    /// `PacketDistributor.sendToServer(msg)`, `ClientPlayNetworking.send(msg)`.
    pub fn send_to_server<P: Payload>(&mut self, msg: &P) {
        self.to_server.push((P::CHANNEL, msg.encode()));
    }

    /// `PacketDistributor.sendToPlayer(player, msg)`, `ServerPlayNetworking.send(player, msg)`.
    pub fn send_to_player<P: Payload>(&mut self, player: u128, msg: &P) {
        self.to_players.push((player, P::CHANNEL, msg.encode()));
    }
}

#[derive(Default)]
pub struct Writer(pub Vec<u8>);

impl Writer {
    pub fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn bool(&mut self, v: bool) {
        self.0.push(u8::from(v));
    }
    pub fn uuid(&mut self, v: u128) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn bytes(&mut self, v: &[u8]) {
        self.0.extend_from_slice(&(v.len() as u32).to_le_bytes());
        self.0.extend_from_slice(v);
    }
    pub fn text(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }
    pub fn pos(&mut self, v: [i32; 3]) {
        v.iter().for_each(|c| self.i32(*c));
    }
    pub fn vec3(&mut self, v: [f64; 3]) {
        v.iter().for_each(|c| self.f64(*c));
    }
}

pub struct Reader<'a>(pub &'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }
    pub fn i32(&mut self) -> Option<i32> {
        self.take().map(i32::from_le_bytes)
    }
    pub fn i64(&mut self) -> Option<i64> {
        self.take().map(i64::from_le_bytes)
    }
    pub fn f32(&mut self) -> Option<f32> {
        self.take().map(f32::from_le_bytes)
    }
    pub fn f64(&mut self) -> Option<f64> {
        self.take().map(f64::from_le_bytes)
    }
    pub fn bool(&mut self) -> Option<bool> {
        self.take::<1>().map(|[b]| b != 0)
    }
    pub fn uuid(&mut self) -> Option<u128> {
        self.take().map(u128::from_le_bytes)
    }
    pub fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = u32::from_le_bytes(self.take()?) as usize;
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head.to_vec())
    }
    pub fn text(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }
    pub fn pos(&mut self) -> Option<[i32; 3]> {
        Some([self.i32()?, self.i32()?, self.i32()?])
    }
    pub fn vec3(&mut self) -> Option<[f64; 3]> {
        Some([self.f64()?, self.f64()?, self.f64()?])
    }
}
"#;

/// ", in `a.B`": where a message's handlers are registered, for its TODO.
fn registered(m: &Message) -> String {
    if m.registered_in.is_empty() {
        String::new()
    } else {
        format!(
            " in {}",
            m.registered_in
                .iter()
                .map(|c| format!("`{c}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// Tests that each message comes back as it was sent.
pub fn net_tests(messages: &[Message]) -> String {
    let mut t = String::new();
    for m in messages {
        let _ = write!(
            t,
            "\n    #[test]\n    fn {snake}_encodes_and_decodes() {{\n        use super::Payload;\n        let msg = super::{name} {{\n",
            snake = m.snake,
            name = m.name
        );
        for (field, ty, _) in &m.fields {
            let _ = writeln!(t, "            {field}: {},", ty.sample());
        }
        let _ = write!(
            t,
            "        }};\n        let data = msg.encode();\n        assert_eq!(super::{name}::decode(&data), Some(msg));\n        assert_eq!(super::{name}::decode(&[data.as_slice(), &[0]].concat()), None);\n    }}\n",
            name = m.name
        );
    }
    t
}

/// A config value, ready to generate.
#[derive(Debug, Clone)]
pub struct Setting {
    /// The setting key Horizon keeps it under: the Java path, `crusher.defaultTime`.
    pub key: String,
    /// The `Config` field: `crusher_default_time`.
    pub field: String,
    /// The button's label: `Default time`.
    pub label: String,
    pub entry: ConfigEntry,
}

/// The settings to generate, from the scan's config.
pub fn settings(config: &[ConfigEntry]) -> Vec<Setting> {
    let mut out: Vec<Setting> = Vec::new();
    for e in config {
        if out.iter().any(|s| s.key == e.path) {
            continue;
        }
        let last = e.path.rsplit('.').next().unwrap_or(&e.path);
        out.push(Setting {
            // Horizon keeps each setting in a file named by its key: safe characters.
            key: e
                .path
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || "._-".contains(c) {
                        c
                    } else {
                        '_'
                    }
                })
                .collect(),
            field: rust_field(&e.path.replace(['.', '-'], "_")),
            label: words(last),
            entry: e.clone(),
        });
    }
    // Labels that clash get their section: `Crusher: Speed`.
    let labels: Vec<String> = out.iter().map(|s| s.label.clone()).collect();
    for s in &mut out {
        if labels.iter().filter(|l| **l == s.label).count() > 1
            && let Some((section, _)) = s.key.rsplit_once('.')
        {
            s.label = format!(
                "{}: {}",
                words(section.rsplit('.').next().unwrap_or(section)),
                s.label
            );
        }
    }
    out
}

/// The Rust type and default expression for a setting.
fn rust_type(s: &Setting) -> (&'static str, String) {
    let e = &s.entry;
    match e.kind {
        ConfigKind::Bool => ("bool", e.default.clone()),
        ConfigKind::Int => ("i32", e.default.clone()),
        ConfigKind::Long => ("i64", e.default.clone()),
        ConfigKind::Double => ("f64", float(&e.default)),
        ConfigKind::String => ("String", format!("{:?}.to_string()", e.default)),
        ConfigKind::Enum => ("&'static str", format!("{:?}", e.default)),
    }
}

fn float(v: &str) -> String {
    if v.contains(['.', 'e', 'E']) {
        v.to_string()
    } else {
        format!("{v}.0")
    }
}

/// A bound as Rust code, if it is a literal.
fn bound(v: Option<&str>, kind: ConfigKind) -> Option<String> {
    let v = v?.trim();
    let ok = match kind {
        ConfigKind::Int => v.parse::<i32>().is_ok(),
        ConfigKind::Long => v.parse::<i64>().is_ok(),
        ConfigKind::Double => v.parse::<f64>().is_ok(),
        _ => false,
    };
    ok.then(|| {
        if kind == ConfigKind::Double {
            float(v)
        } else {
            v.to_string()
        }
    })
}

/// The generated config: the `Config` struct, loading and keeping it, and the
/// settings page.
pub fn config_runtime(settings: &[Setting], java: &str) -> String {
    if settings.is_empty() {
        return String::new();
    }
    let mut r = format!(
        "\n/// The mod's config ({java}). Each value is kept in Horizon's mod settings under\n/// its Java key; players change them on the mod's page in Options > Mods.\n#[derive(Debug, Clone, PartialEq)]\npub struct Config {{\n"
    );
    for s in settings {
        if let Some(c) = &s.entry.comment {
            let _ = writeln!(r, "    /// {}", one_line(c));
        }
        let _ = writeln!(r, "    pub {}: {},", s.field, rust_type(s).0);
    }
    r.push_str("}\n\nimpl Default for Config {\n    fn default() -> Self {\n        Config {\n");
    for s in settings {
        let _ = writeln!(r, "            {}: {},", s.field, rust_type(s).1);
    }
    r.push_str("        }\n    }\n}\n\n/// Each value's key, in the order of its page.\npub const CONFIG_KEYS: &[&str] = &[\n");
    for s in settings {
        let _ = writeln!(r, "    {:?},", s.key);
    }
    r.push_str("];\n");
    for s in settings {
        if s.entry.kind == ConfigKind::Enum {
            let _ = writeln!(
                r,
                "\n/// The values `{}` can take.\npub const {}_VALUES: &[&str] = &[{}];",
                s.key,
                s.field.to_uppercase(),
                s.entry
                    .values
                    .iter()
                    .map(|v| format!("{v:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    // Loading: each value parsed, kept in its range.
    r.push_str("\nimpl Config {\n    /// The config as saved: `get` reads a setting. A value that is missing or\n    /// doesn't parse keeps its default; numbers stay in their range.\n    pub fn load(get: impl Fn(&str) -> Option<String>) -> Self {\n        let mut c = Config::default();\n");
    for s in settings {
        let f = &s.field;
        let key = format!("{:?}", s.key);
        let kind = s.entry.kind;
        let line = match kind {
            ConfigKind::Bool => format!(
                "        if let Some(v) = get({key}).and_then(|v| v.parse().ok()) {{\n            c.{f} = v;\n        }}\n"
            ),
            ConfigKind::Int | ConfigKind::Long | ConfigKind::Double => {
                let min = bound(s.entry.min.as_deref(), kind);
                let max = bound(s.entry.max.as_deref(), kind);
                let clamp = match (min, max) {
                    (Some(lo), Some(hi)) => format!(".clamp({lo}, {hi})"),
                    (Some(lo), None) => format!(".max({lo})"),
                    (None, Some(hi)) => format!(".min({hi})"),
                    (None, None) => String::new(),
                };
                format!(
                    "        if let Some(v) = get({key}).and_then(|v| v.trim().parse::<{ty}>().ok()) {{\n            c.{f} = v{clamp};\n        }}\n",
                    ty = rust_type(s).0
                )
            }
            ConfigKind::String => format!(
                "        if let Some(v) = get({key}) {{\n            c.{f} = v;\n        }}\n"
            ),
            ConfigKind::Enum => format!(
                "        if let Some(v) = get({key}).and_then(|v| {up}_VALUES.iter().find(|x| x.eq_ignore_ascii_case(&v))) {{\n            c.{f} = v;\n        }}\n",
                up = f.to_uppercase()
            ),
        };
        r.push_str(&line);
    }
    r.push_str("        c\n    }\n\n    /// Each value as it is saved, by key.\n    pub fn saved(&self) -> Vec<(&'static str, String)> {\n        vec![\n");
    for s in settings {
        let _ = writeln!(
            r,
            "            ({:?}, self.{}.to_string()),",
            s.key, s.field
        );
    }
    r.push_str("        ]\n    }\n}\n");
    r.push_str(CONFIG_STATE);

    // The settings page.
    r.push_str("\n/// The labels of the buttons on the mod's settings page (Options > Mods), one per\n/// config value. `get` reads a saved setting.\npub fn settings_buttons(get: impl Fn(&str) -> Option<String>) -> Vec<String> {\n    let c = load_config(get);\n    vec![\n");
    for s in settings {
        let f = &s.field;
        let value = match s.entry.kind {
            ConfigKind::Bool => format!("if c.{f} {{ \"On\" }} else {{ \"Off\" }}"),
            ConfigKind::Enum => format!("words(c.{f})"),
            _ => format!("c.{f}"),
        };
        let _ = writeln!(r, "        format!(\"{}: {{}}\", {value}),", s.label);
    }
    r.push_str("    ]\n}\n\n/// A settings button was clicked: the value it changes, as (key, value) to save.\n/// On and off toggle, a choice moves to the next one, a number steps through its\n/// range and starts over.\npub fn settings_click(button: usize, get: impl Fn(&str) -> Option<String>) -> Option<(String, String)> {\n");
    // Only text values: no button changes anything (they're edited in their files).
    if settings.iter().all(|s| s.entry.kind == ConfigKind::String) {
        r.push_str("    let _ = (button, get);\n    None\n}\n");
        r.push_str(CONFIG_HELPERS);
        return r;
    }
    r.push_str("    let mut c = load_config(get);\n    match button {\n");
    for (i, s) in settings.iter().enumerate() {
        let f = &s.field;
        let kind = s.entry.kind;
        let change = match kind {
            ConfigKind::Bool => format!("c.{f} = !c.{f}"),
            ConfigKind::Enum => format!(
                "{{\n            let values = {up}_VALUES;\n            let i = values.iter().position(|v| *v == c.{f}).unwrap_or(0);\n            c.{f} = values[(i + 1) % values.len()];\n        }}",
                up = f.to_uppercase()
            ),
            ConfigKind::Int | ConfigKind::Long | ConfigKind::Double => {
                let ty = rust_type(s).0;
                let min = bound(s.entry.min.as_deref(), kind);
                let max = bound(s.entry.max.as_deref(), kind);
                match (min, max) {
                    (Some(lo), Some(hi)) => format!(
                        "{{\n            let step = step({lo} as f64, {hi} as f64) as {ty};\n            let next = c.{f} + step;\n            c.{f} = if next > {hi} {{ {lo} }} else {{ next }};\n        }}"
                    ),
                    _ => format!(
                        "c.{f} += {}",
                        if kind == ConfigKind::Double {
                            "0.1"
                        } else {
                            "1"
                        }
                    ),
                }
            }
            // Text is edited in the setting's file.
            ConfigKind::String => "return None".to_string(),
        };
        let _ = writeln!(r, "        {i} => {change},");
    }
    r.push_str("        _ => return None,\n    }\n    let (key, value) = c\n        .saved()\n        .into_iter()\n        .nth(button)?;\n    set_config(c);\n    Some((key.to_string(), value))\n}\n");
    r.push_str(CONFIG_HELPERS);
    r
}

const CONFIG_STATE: &str = r#"
static CONFIG: std::sync::Mutex<Option<Config>> = std::sync::Mutex::new(None);

/// The config, as last loaded (its defaults until a hook loads it). Read it from
/// any ported code: `config().crusher_sounds`.
pub fn config() -> Config {
    CONFIG
        .lock()
        .ok()
        .and_then(|c| c.clone())
        .unwrap_or_default()
}

/// Loads the config from the saved settings and keeps it for `config()`.
pub fn load_config(get: impl Fn(&str) -> Option<String>) -> Config {
    let c = Config::load(get);
    set_config(c.clone());
    c
}

/// Loads the config once, the first time a hook can read the settings.
pub fn ensure_config(get: impl Fn(&str) -> Option<String>) {
    let loaded = CONFIG.lock().map(|c| c.is_some()).unwrap_or(true);
    if !loaded {
        load_config(get);
    }
}

fn set_config(c: Config) {
    if let Ok(mut slot) = CONFIG.lock() {
        *slot = Some(c);
    }
}
"#;

const CONFIG_HELPERS: &str = r#"
/// A step through a range: about a tenth of it, rounded to 1, 2 or 5 times a power
/// of ten (10..1200 steps by 100).
#[allow(dead_code)]
fn step(min: f64, max: f64) -> f64 {
    let raw = ((max - min) / 10.0).max(f64::MIN_POSITIVE);
    let p = 10f64.powf(raw.log10().floor());
    let m = raw / p;
    let nice = if m < 1.5 { 1.0 } else if m < 3.5 { 2.0 } else if m < 7.5 { 5.0 } else { 10.0 };
    (nice * p).max(if min.fract() == 0.0 && max.fract() == 0.0 { 1.0 } else { 0.0 })
}

/// `VERY_LOUD` → `Very loud`.
#[allow(dead_code)]
fn words(constant: &str) -> String {
    let lower = constant.replace('_', " ").to_lowercase();
    let mut chars = lower.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}
"#;

/// Tests for the config: defaults load, bad values keep defaults, every button
/// changes its value and saves under its key.
pub fn config_tests(settings: &[Setting]) -> String {
    if settings.is_empty() {
        return String::new();
    }
    format!(
        r#"
    #[test]
    fn config_loads_and_keeps_defaults() {{
        let c = super::Config::load(|_| None);
        assert_eq!(c, super::Config::default());
        let bad = super::Config::load(|_| Some("not a value".to_string()));
        assert_eq!(super::settings_buttons(|_| None).len(), super::CONFIG_KEYS.len());
        let _ = bad;
    }}

    #[test]
    fn settings_buttons_save_their_values() {{
        for (i, key) in super::CONFIG_KEYS.iter().enumerate() {{
            if let Some((saved, value)) = super::settings_click(i, |_| None) {{
                assert_eq!(saved, *key);
                // What was saved loads back.
                let c = super::Config::load(|k| (k == saved).then(|| value.clone()));
                assert!(c.saved().contains(&(*key, value.clone())), "{{key}} = {{value}}");
            }}
        }}
        assert_eq!(super::settings_click({n}, |_| None), None);
    }}
"#,
        n = settings.len()
    )
}

/// `SetSpeed` → `set_speed`.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A Java name as a Rust field: `defaultTime` → `default_time`, keywords escaped.
fn rust_field(name: &str) -> String {
    // Only letters, digits and single underscores (a config key can be a sentence:
    // `how many times a villager can heal a guard's equipment`).
    let cleaned: String = snake(name)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let mut s = cleaned
        .split('_')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert_str(0, "v_");
    }
    const KEYWORDS: &[&str] = &[
        "type", "match", "move", "ref", "fn", "mod", "use", "loop", "impl", "trait", "self",
        "crate", "super", "where", "async", "await", "dyn", "box", "yield",
    ];
    if KEYWORDS.contains(&s.as_str()) {
        format!("r#{s}")
    } else {
        s
    }
}

/// `defaultTime` → `Default time`.
fn words(name: &str) -> String {
    let s = snake(name).replace('_', " ");
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(snake("SetSpeed"), "set_speed");
        assert_eq!(rust_field("defaultTime"), "default_time");
        assert_eq!(rust_field("type"), "r#type");
        assert_eq!(
            rust_field("villager stuff_how many times a guard's"),
            "villager_stuff_how_many_times_a_guard_s"
        );
        assert_eq!(rust_field("2fast"), "v_2fast");
        assert_eq!(words("defaultTime"), "Default time");
    }
}
