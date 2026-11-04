// A minimal Java class file reader: the class's name, superclass, interfaces, method
// count and every class it refers to (from the constant pool and descriptors).

use std::collections::BTreeSet;

use anyhow::{Result, bail};

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
}

pub(crate) const ACC_INTERFACE: u16 = 0x0200;
pub(crate) const ACC_ANNOTATION: u16 = 0x2000;
pub(crate) const ACC_ENUM: u16 = 0x4000;

enum Constant {
    Utf8(String),
    Class(u16),
    NameAndType(u16),
    Other,
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
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
            3 | 4 => {
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
    let utf8 = |i: u16| match pool.get(i as usize) {
        Some(Constant::Utf8(s)) => Some(s.as_str()),
        _ => None,
    };
    let class_name = |i: u16| match pool.get(i as usize) {
        Some(Constant::Class(n)) => utf8(*n),
        _ => None,
    };

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
    for c in &pool {
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
    for table in 0..2 {
        let n = r.u16()?;
        for _ in 0..n {
            r.u16()?; // access
            r.u16()?; // name
            if let Some(desc) = utf8(r.u16()?) {
                descriptor_classes(desc, &mut references);
            }
            let attributes = r.u16()?;
            for _ in 0..attributes {
                r.u16()?;
                let len = r.u32()? as usize;
                r.take(len)?;
            }
        }
        if table == 1 {
            methods = n as usize;
        }
    }

    let has_mixin_annotation = pool
        .iter()
        .any(|c| matches!(c, Constant::Utf8(s) if s == "Lorg/spongepowered/asm/mixin/Mixin;"));
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
    })
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
