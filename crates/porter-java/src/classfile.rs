// A minimal Java class file reader: the class's name, superclass, interfaces, method
// count and every class it refers to (from the constant pool and descriptors). For
// Mixin classes it also decodes the annotations on the class and its members, which
// Mixin keeps in the class file.

use std::collections::BTreeSet;

use anyhow::{Result, bail};

use crate::source::{Annotation, Member, Value};

pub(crate) struct ClassFile {
    /// Dotted name, nested classes joined with `.`: `com.demo.Foo.Bar`.
    pub name: String,
    /// The top-level class it belongs to: `com.demo.Foo` (itself when top-level).
    pub outer: String,
    pub super_name: Option<String>,
    pub interfaces: Vec<String>,
    pub access: u16,
    pub methods: usize,
    pub references: BTreeSet<String>,
    /// Whether the class carries `@Mixin` (it keeps the annotation in the class file).
    pub has_mixin_annotation: bool,
    /// For Mixin classes: the class's annotations.
    pub annotations: Vec<Annotation>,
    /// For Mixin classes: its fields and methods with their annotations, without
    /// synthetic members and static initializers.
    pub members: Vec<Member>,
}

pub(crate) const ACC_INTERFACE: u16 = 0x0200;
pub(crate) const ACC_ANNOTATION: u16 = 0x2000;
pub(crate) const ACC_ENUM: u16 = 0x4000;
const ACC_SYNTHETIC: u16 = 0x1000;

const MIXIN_DESCRIPTOR: &str = "Lorg/spongepowered/asm/mixin/Mixin;";

enum Constant {
    Utf8(String),
    Integer(i32),
    Class(u16),
    NameAndType(u16),
    Other,
}

struct Pool(Vec<Constant>);

impl Pool {
    fn utf8(&self, i: u16) -> Option<&str> {
        match self.0.get(i as usize) {
            Some(Constant::Utf8(s)) => Some(s),
            _ => None,
        }
    }
    fn class_name(&self, i: u16) -> Option<&str> {
        match self.0.get(i as usize) {
            Some(Constant::Class(n)) => self.utf8(*n),
            _ => None,
        }
    }
    fn int(&self, i: u16) -> Option<i32> {
        match self.0.get(i as usize) {
            Some(Constant::Integer(v)) => Some(*v),
            _ => None,
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let Some(slice) = self.bytes.get(self.pos..self.pos + n) else {
            bail!("class file ends early");
        };
        self.pos += n;
        Ok(slice)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

pub(crate) fn parse(bytes: &[u8]) -> Result<ClassFile> {
    let mut r = Reader { bytes, pos: 0 };
    if r.u32()? != 0xCAFE_BABE {
        bail!("not a class file");
    }
    r.u16()?; // minor
    r.u16()?; // major

    let count = r.u16()? as usize;
    let mut pool: Vec<Constant> = Vec::with_capacity(count);
    pool.push(Constant::Other); // index 0 is unused
    while pool.len() < count {
        let tag = r.u8()?;
        let constant = match tag {
            1 => {
                let len = r.u16()? as usize;
                Constant::Utf8(String::from_utf8_lossy(r.take(len)?).into_owned())
            }
            7 => Constant::Class(r.u16()?),
            12 => {
                r.u16()?; // name
                Constant::NameAndType(r.u16()?)
            }
            3 => {
                let b = r.take(4)?;
                Constant::Integer(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            }
            4 => {
                r.take(4)?;
                Constant::Other
            }
            5 | 6 => {
                r.take(8)?;
                pool.push(Constant::Other);
                Constant::Other // longs and doubles take two slots
            }
            8 | 16 | 19 | 20 => {
                r.u16()?;
                Constant::Other
            }
            9..=11 | 17 | 18 => {
                r.take(4)?;
                Constant::Other
            }
            15 => {
                r.take(3)?;
                Constant::Other
            }
            other => bail!("unknown constant pool tag {other}"),
        };
        pool.push(constant);
    }
    let pool = Pool(pool);
    let utf8 = |i: u16| pool.utf8(i);
    let class_name = |i: u16| pool.class_name(i);
    let has_mixin_annotation = pool
        .0
        .iter()
        .any(|c| matches!(c, Constant::Utf8(s) if s == MIXIN_DESCRIPTOR));

    let access = r.u16()?;
    let this = class_name(r.u16()?).unwrap_or("").to_string();
    let super_index = r.u16()?;
    let super_name = (super_index != 0)
        .then(|| class_name(super_index))
        .flatten()
        .map(dotted);
    let interface_count = r.u16()?;
    let mut interfaces = Vec::new();
    for _ in 0..interface_count {
        if let Some(n) = class_name(r.u16()?) {
            interfaces.push(dotted(n));
        }
    }

    let mut references = BTreeSet::new();
    for c in &pool.0 {
        match c {
            Constant::Class(n) => {
                if let Some(name) = utf8(*n) {
                    if name.starts_with('[') {
                        descriptor_classes(name, &mut references);
                    } else {
                        references.insert(dotted(name));
                    }
                }
            }
            Constant::NameAndType(d) => {
                if let Some(desc) = utf8(*d) {
                    descriptor_classes(desc, &mut references);
                }
            }
            _ => {}
        }
    }

    let mut methods = 0;
    let mut members = Vec::new();
    for table in 0..2 {
        let is_method = table == 1;
        let n = r.u16()?;
        for _ in 0..n {
            let member_access = r.u16()?;
            let name = utf8(r.u16()?).unwrap_or("").to_string();
            if let Some(desc) = utf8(r.u16()?) {
                descriptor_classes(desc, &mut references);
            }
            let annotations = read_attributes(&mut r, &pool, has_mixin_annotation)?;
            let keep = has_mixin_annotation
                && member_access & ACC_SYNTHETIC == 0
                && name != "<clinit>"
                && !name.starts_with("lambda$");
            if keep {
                members.push(Member {
                    is_constructor: name == "<init>",
                    name,
                    is_method,
                    annotations,
                    ty: String::new(),
                    is_static: member_access & 0x0008 != 0,
                    is_final: member_access & 0x0010 != 0,
                    init: None,
                });
            }
        }
        if is_method {
            methods = n as usize;
        }
    }
    let annotations = read_attributes(&mut r, &pool, has_mixin_annotation)?;

    let name = dotted(&this);
    let outer = dotted(this.split('$').next().unwrap_or(&this));
    references.remove(&name);
    Ok(ClassFile {
        name,
        outer,
        super_name,
        interfaces,
        access,
        methods,
        references,
        has_mixin_annotation,
        annotations,
        members,
    })
}

/// Reads an attribute table. When `decode` is set, returns the annotations in its
/// `Runtime(In)visibleAnnotations` attributes; otherwise skips everything.
fn read_attributes(r: &mut Reader, pool: &Pool, decode: bool) -> Result<Vec<Annotation>> {
    let mut out = Vec::new();
    let count = r.u16()?;
    for _ in 0..count {
        let name = pool.utf8(r.u16()?).unwrap_or("");
        let len = r.u32()? as usize;
        let data = r.take(len)?;
        if decode
            && matches!(
                name,
                "RuntimeInvisibleAnnotations" | "RuntimeVisibleAnnotations"
            )
        {
            let mut a = Reader {
                bytes: data,
                pos: 0,
            };
            for _ in 0..a.u16()? {
                out.push(read_annotation(&mut a, pool)?);
            }
        }
    }
    Ok(out)
}

fn read_annotation(r: &mut Reader, pool: &Pool) -> Result<Annotation> {
    let name = descriptor_name(pool.utf8(r.u16()?).unwrap_or(""));
    let mut args = Vec::new();
    for _ in 0..r.u16()? {
        let key = pool.utf8(r.u16()?).unwrap_or("").to_string();
        args.push((key, read_value(r, pool)?));
    }
    Ok(Annotation { name, args })
}

fn read_value(r: &mut Reader, pool: &Pool) -> Result<Value> {
    let tag = r.u8()?;
    Ok(match tag {
        b's' => Value::Str(pool.utf8(r.u16()?).unwrap_or("").to_string()),
        b'Z' => Value::Bool(pool.int(r.u16()?).unwrap_or(0) != 0),
        b'B' | b'C' | b'I' | b'S' => Value::Other(
            pool.int(r.u16()?)
                .map(|v| v.to_string())
                .unwrap_or_default(),
        ),
        b'D' | b'F' | b'J' => {
            r.u16()?;
            Value::Other(String::new())
        }
        // Enum constant: `At.Shift.AFTER` becomes `org...At.Shift.AFTER`.
        b'e' => {
            let ty = descriptor_name(pool.utf8(r.u16()?).unwrap_or(""));
            let constant = pool.utf8(r.u16()?).unwrap_or("");
            Value::Name(format!("{ty}.{constant}"))
        }
        b'c' => Value::Class(descriptor_name(pool.utf8(r.u16()?).unwrap_or(""))),
        b'@' => Value::Annotation(read_annotation(r, pool)?),
        b'[' => {
            let n = r.u16()?;
            let mut items = Vec::with_capacity(n as usize);
            for _ in 0..n {
                items.push(read_value(r, pool)?);
            }
            Value::Array(items)
        }
        other => bail!("unknown annotation value tag {}", other as char),
    })
}

/// `Lnet/minecraft/world/level/Level;` → `net.minecraft.world.level.Level`.
fn descriptor_name(desc: &str) -> String {
    match desc.strip_prefix('L').and_then(|d| d.strip_suffix(';')) {
        Some(internal) => dotted(internal),
        None => desc.to_string(),
    }
}

/// Adds the classes named in a descriptor such as `(Lnet/minecraft/world/level/Level;I)V`.
fn descriptor_classes(desc: &str, out: &mut BTreeSet<String>) {
    let mut rest = desc;
    while let Some(start) = rest.find('L') {
        let after = &rest[start + 1..];
        let Some(end) = after.find(';') else { break };
        let name = &after[..end];
        if !name.is_empty() && !name.contains(['(', ')', '[', '<']) {
            out.insert(dotted(name));
        }
        rest = &after[end + 1..];
    }
}

/// `net/minecraft/Foo$Bar$1` → `net.minecraft.Foo.Bar` (anonymous classes dropped).
pub(crate) fn dotted(internal: &str) -> String {
    let path = internal.replace('/', ".");
    let mut parts = path.split('$');
    let mut out = parts.next().unwrap_or("").to_string();
    for part in parts {
        if part.is_empty() || part.starts_with(|c: char| c.is_ascii_digit()) {
            break;
        }
        out.push('.');
        out.push_str(part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_internal_names() {
        assert_eq!(dotted("net/minecraft/Foo$Bar$1"), "net.minecraft.Foo.Bar");
        assert_eq!(dotted("a/B$1$C"), "a.B");
    }

    #[test]
    fn reads_descriptor_classes() {
        let mut out = BTreeSet::new();
        descriptor_classes(
            "(Lnet/minecraft/world/level/Level;I[Ljava/lang/String;)V",
            &mut out,
        );
        assert!(out.contains("net.minecraft.world.level.Level"));
        assert!(out.contains("java.lang.String"));
        assert_eq!(out.len(), 2);
    }

    /// A hand-built class file: `class demo/A extends java/lang/Object` with no members.
    #[test]
    fn parses_a_minimal_class() {
        let mut b: Vec<u8> = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 65];
        b.extend_from_slice(&5u16.to_be_bytes()); // pool count: 4 entries
        b.push(1);
        b.extend_from_slice(&6u16.to_be_bytes());
        b.extend_from_slice(b"demo/A");
        b.push(7);
        b.extend_from_slice(&1u16.to_be_bytes());
        b.push(1);
        b.extend_from_slice(&16u16.to_be_bytes());
        b.extend_from_slice(b"java/lang/Object");
        b.push(7);
        b.extend_from_slice(&3u16.to_be_bytes());
        b.extend_from_slice(&0x0021u16.to_be_bytes()); // access
        b.extend_from_slice(&2u16.to_be_bytes()); // this
        b.extend_from_slice(&4u16.to_be_bytes()); // super
        b.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]); // interfaces, fields, methods, attributes
        let c = parse(&b).unwrap();
        assert_eq!(c.name, "demo.A");
        assert_eq!(c.super_name.as_deref(), Some("java.lang.Object"));
        assert!(c.references.contains("java.lang.Object"));
        assert!(!c.references.contains("demo.A"));
        assert_eq!(c.methods, 0);
    }
}
