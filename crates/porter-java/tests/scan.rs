// End-to-end scans of the fixture mods in tests/fixtures and of a jar built here.

use std::io::Write;
use std::path::{Path, PathBuf};

use porter_model::{InputKind, Loader, ModScan, Side};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn mixin_verdicts(scan: &ModScan, class: &str) -> Vec<(String, String)> {
    scan.mixins
        .iter()
        .find(|m| m.name == class)
        .unwrap_or_else(|| panic!("no Mixin {class}"))
        .members
        .iter()
        .map(|m| (m.annotation.clone(), m.verdict.name().to_string()))
        .collect()
}

#[test]
fn scans_a_neoforge_source_tree() {
    let scan = porter_java::scan(&fixture("neoforge-demo")).unwrap();
    assert_eq!(scan.input_kind, InputKind::SourceTree);
    assert!(scan.warnings.is_empty(), "warnings: {:?}", scan.warnings);

    // Metadata from a template, with Gradle properties substituted.
    let [meta] = scan.metadata.as_slice() else {
        panic!("expected one metadata file, got {:?}", scan.metadata);
    };
    assert_eq!(meta.loader, Loader::NeoForge);
    assert_eq!(meta.id.as_deref(), Some("demo"));
    assert_eq!(meta.name.as_deref(), Some("Demo Machines"));
    assert_eq!(meta.version.as_deref(), Some("0.3.0"));
    assert_eq!(meta.license.as_deref(), Some("MIT"));
    assert_eq!(meta.minecraft.as_deref(), Some("[26.3]"));
    assert_eq!(meta.dependencies, vec!["flywheel"]);

    // src/test and build/ are skipped; a package named `target` is not.
    let mut names: Vec<&str> = scan.classes.iter().map(|c| c.name.as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "com.demo.Demo",
            "com.demo.ModItems",
            "com.demo.SpeedHolder",
            "com.demo.Xplat",
            "com.demo.block.CrankBlock",
            "com.demo.block.CrankBlockEntity",
            "com.demo.mixin.LevelMixin",
            "com.demo.mixin.client.GuiMixin",
            "com.demo.target.Aim",
        ]
    );
    let crank = scan
        .classes
        .iter()
        .find(|c| c.name == "com.demo.block.CrankBlock")
        .unwrap();
    assert_eq!(
        crank.superclass.as_deref(),
        Some("net.minecraft.world.level.block.Block")
    );
    assert_eq!(
        crank.interfaces,
        vec!["net.minecraft.world.level.block.EntityBlock"]
    );
    let be = scan
        .classes
        .iter()
        .find(|c| c.name == "com.demo.block.CrankBlockEntity")
        .unwrap();
    assert!(be.references.contains(&"com.demo.Demo".to_string()));
    assert!(
        be.references
            .contains(&"net.minecraft.nbt.CompoundTag".to_string())
    );

    // Mixins: targets resolved through imports, sides from the config.
    let level = scan
        .mixins
        .iter()
        .find(|m| m.name == "com.demo.mixin.LevelMixin")
        .unwrap();
    assert_eq!(level.targets, vec!["net.minecraft.world.level.Level"]);
    assert_eq!(level.side, Side::Common);
    assert_eq!(
        mixin_verdicts(&scan, "com.demo.mixin.LevelMixin"),
        vec![
            ("Shadow".to_string(), "not needed".to_string()),
            ("Inject".to_string(), "supported".to_string()),
            ("Inject".to_string(), "partial".to_string()),
            ("Redirect".to_string(), "blocked".to_string()),
            ("Redirect".to_string(), "partial".to_string()),
        ]
    );
    let gui = scan
        .mixins
        .iter()
        .find(|m| m.name == "com.demo.mixin.client.GuiMixin")
        .unwrap();
    assert_eq!(gui.side, Side::Client);
    assert_eq!(gui.targets, vec!["net.minecraft.client.gui.Gui"]);
    assert_eq!(
        mixin_verdicts(&scan, "com.demo.mixin.client.GuiMixin"),
        vec![
            ("Unique".to_string(), "supported".to_string()),
            ("ModifyArg".to_string(), "supported".to_string()),
            ("merged".to_string(), "supported".to_string()),
            ("implements".to_string(), "blocked".to_string()),
        ]
    );

    // Registrations through DeferredRegister fields, and through helper methods.
    let mut regs: Vec<(String, Option<String>, String)> = scan
        .registrations
        .iter()
        .map(|r| (r.registry.clone(), r.id.clone(), r.via.clone()))
        .collect();
    regs.sort();
    let entry = |registry: &str, id: Option<&str>, via: &str| {
        (
            registry.to_string(),
            id.map(str::to_string),
            via.to_string(),
        )
    };
    assert_eq!(
        regs,
        vec![
            entry("block", Some("brass_anvil"), "BiConsumer"),
            entry("block", Some("brass_bell"), "BiConsumer"),
            entry("block", Some("crank"), "DeferredRegister"),
            entry("block", Some("crank_2"), "DeferredRegister"),
            entry("block_entity_type", Some("crank"), "DeferredRegister"),
            entry("item", None, "helper method"),
            entry("item", Some("axle"), "helper method"),
            entry("item", Some("gear"), "helper method"),
            entry("item", Some("handle"), "DeferredRegister"),
            entry("item", Some("name"), "DeferredRegister"),
        ]
    );

    // What each registration creates, and its arguments to read properties from.
    let find = |registry: &str, id: &str| {
        scan.registrations
            .iter()
            .find(|r| r.registry == registry && r.id.as_deref() == Some(id))
            .unwrap()
    };
    let crank = find("block", "crank");
    assert_eq!(crank.creates.as_deref(), Some("CrankBlock"));
    assert!(crank.args.contains("ofFullCopy(Blocks.IRON_BLOCK)"));
    assert_eq!(
        find("block", "crank_2").creates.as_deref(),
        Some("CrankBlock")
    );
    assert_eq!(
        find("block_entity_type", "crank").creates.as_deref(),
        Some("CrankBlockEntity")
    );

    // The block entity's fields, methods and saved values.
    let be = scan
        .classes
        .iter()
        .find(|c| c.name == "com.demo.block.CrankBlockEntity")
        .unwrap();
    let fields: Vec<(&str, &str, bool)> = be
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.ty.as_str(), f.is_static))
        .collect();
    assert_eq!(
        fields,
        vec![
            ("MAX_SPEED", "float", true),
            ("speed", "float", false),
            ("turns", "int", false),
            ("powered", "boolean", false),
            ("owner", "String", false),
        ]
    );
    assert!(be.method_names.contains(&"tick".to_string()));
    let saved: Vec<(&str, &str, &str)> = be
        .saved
        .iter()
        .map(|s| (s.key.as_str(), s.method.as_str(), s.value.as_str()))
        .collect();
    assert_eq!(
        saved,
        vec![
            ("Speed", "putFloat", "speed"),
            ("Turns", "putInt", "turns"),
            ("Powered", "putBoolean", "powered"),
            ("Owner", "putString", "owner"),
        ]
    );

    // Resources from both resource roots.
    let r = &scan.resources;
    assert_eq!(r.assets(), 5);
    assert_eq!(r.data(), 2);
    assert_eq!(r.categories["data/tags/block"], 1);
    assert_eq!(r.namespaces, vec!["demo", "minecraft"]);
    assert_eq!(
        r.roots,
        vec!["src/generated/resources", "src/main/resources"]
    );
}

#[test]
fn scans_a_fabric_source_tree() {
    let scan = porter_java::scan(&fixture("fabric-demo")).unwrap();
    let [meta] = scan.metadata.as_slice() else {
        panic!("expected one metadata file");
    };
    assert_eq!(meta.loader, Loader::Fabric);
    assert_eq!(meta.license.as_deref(), Some("MIT, CC0-1.0"));
    assert_eq!(meta.minecraft.as_deref(), Some("~26.3"));
    assert_eq!(meta.dependencies, vec!["cloth-config"]);
    // No gradle.properties: the placeholder stays visible rather than guessed.
    assert_eq!(meta.version.as_deref(), Some("${version}"));

    assert_eq!(scan.classes.len(), 3);
    let client = scan
        .classes
        .iter()
        .find(|c| c.name == "net.fdemo.client.FabricDemoClient")
        .unwrap();
    assert_eq!(client.side, Side::Client);

    let accessor = &scan.mixins[0];
    assert_eq!(
        accessor.targets,
        vec!["net.minecraft.world.entity.player.Player"]
    );
    let kinds: Vec<_> = accessor
        .members
        .iter()
        .map(|m| m.horizon_kind.clone().unwrap_or_default())
        .collect();
    assert_eq!(kinds, vec!["GET/SET", "INVOKE"]);

    assert_eq!(scan.registrations.len(), 1);
    assert_eq!(scan.registrations[0].registry, "item");
    assert_eq!(scan.registrations[0].id.as_deref(), Some("gem"));
    assert_eq!(scan.registrations[0].via, "Registry.register");
}

/// A class file with a superclass, extra class constants and extra UTF-8 constants.
fn class_bytes(this: &str, superclass: &str, classes: &[&str], utf8: &[&str]) -> Vec<u8> {
    let mut pool: Vec<Vec<u8>> = Vec::new();
    let add_class = |pool: &mut Vec<Vec<u8>>, name: &str| -> u16 {
        let mut u = vec![1];
        u.extend_from_slice(&(name.len() as u16).to_be_bytes());
        u.extend_from_slice(name.as_bytes());
        pool.push(u);
        let utf8_index = pool.len() as u16;
        let mut c = vec![7];
        c.extend_from_slice(&utf8_index.to_be_bytes());
        pool.push(c);
        pool.len() as u16
    };
    let this_index = add_class(&mut pool, this);
    let super_index = add_class(&mut pool, superclass);
    for c in classes {
        add_class(&mut pool, c);
    }
    for s in utf8 {
        let mut u = vec![1];
        u.extend_from_slice(&(s.len() as u16).to_be_bytes());
        u.extend_from_slice(s.as_bytes());
        pool.push(u);
    }
    let mut b = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 65];
    b.extend_from_slice(&(pool.len() as u16 + 1).to_be_bytes());
    for entry in pool {
        b.extend_from_slice(&entry);
    }
    b.extend_from_slice(&0x0021u16.to_be_bytes());
    b.extend_from_slice(&this_index.to_be_bytes());
    b.extend_from_slice(&super_index.to_be_bytes());
    b.extend_from_slice(&[0; 8]); // no interfaces, fields, methods or attributes
    b
}

/// Writes class files whose members carry annotations, as compiled Mixins do.
#[derive(Default)]
struct ClassWriter {
    pool: Vec<Vec<u8>>,
}

impl ClassWriter {
    fn push(&mut self, entry: Vec<u8>) -> u16 {
        self.pool.push(entry);
        self.pool.len() as u16
    }
    fn utf8(&mut self, s: &str) -> u16 {
        let mut u = vec![1];
        u.extend_from_slice(&(s.len() as u16).to_be_bytes());
        u.extend_from_slice(s.as_bytes());
        self.push(u)
    }
    fn class(&mut self, internal: &str) -> u16 {
        let name = self.utf8(internal);
        let mut c = vec![7];
        c.extend_from_slice(&name.to_be_bytes());
        self.push(c)
    }
    fn int(&mut self, v: i32) -> u16 {
        let mut c = vec![3];
        c.extend_from_slice(&v.to_be_bytes());
        self.push(c)
    }
    fn tagged(&mut self, tag: u8, index: u16) -> Vec<u8> {
        let mut v = vec![tag];
        v.extend_from_slice(&index.to_be_bytes());
        v
    }
    fn string(&mut self, s: &str) -> Vec<u8> {
        let i = self.utf8(s);
        self.tagged(b's', i)
    }
    fn boolean(&mut self, b: bool) -> Vec<u8> {
        let i = self.int(b as i32);
        self.tagged(b'Z', i)
    }
    fn class_value(&mut self, descriptor: &str) -> Vec<u8> {
        let i = self.utf8(descriptor);
        self.tagged(b'c', i)
    }
    fn array(values: Vec<Vec<u8>>) -> Vec<u8> {
        let mut v = vec![b'['];
        v.extend_from_slice(&(values.len() as u16).to_be_bytes());
        values.into_iter().for_each(|x| v.extend(x));
        v
    }
    fn nested(annotation: Vec<u8>) -> Vec<u8> {
        let mut v = vec![b'@'];
        v.extend(annotation);
        v
    }
    fn annotation(&mut self, descriptor: &str, pairs: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
        let ty = self.utf8(descriptor);
        let mut v = ty.to_be_bytes().to_vec();
        v.extend_from_slice(&(pairs.len() as u16).to_be_bytes());
        for (key, value) in pairs {
            let k = self.utf8(key);
            v.extend_from_slice(&k.to_be_bytes());
            v.extend(value);
        }
        v
    }
    /// An attribute table holding one RuntimeInvisibleAnnotations attribute.
    fn annotations(&mut self, annotations: Vec<Vec<u8>>) -> Vec<u8> {
        if annotations.is_empty() {
            return vec![0, 0];
        }
        let name = self.utf8("RuntimeInvisibleAnnotations");
        let mut body = (annotations.len() as u16).to_be_bytes().to_vec();
        annotations.into_iter().for_each(|a| body.extend(a));
        let mut v = 1u16.to_be_bytes().to_vec();
        v.extend_from_slice(&name.to_be_bytes());
        v.extend_from_slice(&(body.len() as u32).to_be_bytes());
        v.extend(body);
        v
    }
    /// A field or method: access flags, name, descriptor and its attribute table.
    fn member(
        &mut self,
        access: u16,
        name: &str,
        descriptor: &str,
        attributes: Vec<u8>,
    ) -> Vec<u8> {
        let n = self.utf8(name);
        let d = self.utf8(descriptor);
        let mut v = access.to_be_bytes().to_vec();
        v.extend_from_slice(&n.to_be_bytes());
        v.extend_from_slice(&d.to_be_bytes());
        v.extend(attributes);
        v
    }
    fn finish(
        self,
        this: u16,
        superclass: u16,
        fields: Vec<Vec<u8>>,
        methods: Vec<Vec<u8>>,
        class_attributes: Vec<u8>,
    ) -> Vec<u8> {
        let mut b = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 65];
        b.extend_from_slice(&(self.pool.len() as u16 + 1).to_be_bytes());
        self.pool.into_iter().for_each(|e| b.extend(e));
        b.extend_from_slice(&0x0021u16.to_be_bytes());
        b.extend_from_slice(&this.to_be_bytes());
        b.extend_from_slice(&superclass.to_be_bytes());
        b.extend_from_slice(&0u16.to_be_bytes()); // interfaces
        for table in [fields, methods] {
            b.extend_from_slice(&(table.len() as u16).to_be_bytes());
            table.into_iter().for_each(|m| b.extend(m));
        }
        b.extend(class_attributes);
        b
    }
}

/// A compiled Mixin into Level: `@Shadow int x`, a constructor, an `@Inject` at HEAD
/// (cancellable) and a synthetic lambda.
fn mixin_class_bytes() -> Vec<u8> {
    let mut w = ClassWriter::default();
    let this = w.class("jdemo/mixin/FooMixin");
    let superclass = w.class("java/lang/Object");

    let level = w.class_value("Lnet/minecraft/world/level/Level;");
    let mixin = w.annotation(
        "Lorg/spongepowered/asm/mixin/Mixin;",
        vec![("value", ClassWriter::array(vec![level]))],
    );
    let class_attributes = w.annotations(vec![mixin]);

    let shadow = w.annotation("Lorg/spongepowered/asm/mixin/Shadow;", vec![]);
    let shadow_attrs = w.annotations(vec![shadow]);
    let field = w.member(0x0002, "x", "I", shadow_attrs);

    let no_attrs = w.annotations(vec![]);
    let constructor = w.member(0x0001, "<init>", "()V", no_attrs);

    let head = w.string("HEAD");
    let at = w.annotation(
        "Lorg/spongepowered/asm/mixin/injection/At;",
        vec![("value", head)],
    );
    let tick = w.string("tick");
    let cancellable = w.boolean(true);
    let inject = w.annotation(
        "Lorg/spongepowered/asm/mixin/injection/Inject;",
        vec![
            ("method", ClassWriter::array(vec![tick])),
            ("at", ClassWriter::nested(at)),
            ("cancellable", cancellable),
        ],
    );
    let inject_attrs = w.annotations(vec![inject]);
    let injector = w.member(
        0x0002,
        "jdemo$tick",
        "(Lorg/spongepowered/asm/mixin/injection/callback/CallbackInfo;)V",
        inject_attrs,
    );
    let no_attrs = w.annotations(vec![]);
    let lambda = w.member(0x1002, "lambda$jdemo$tick$0", "()V", no_attrs);

    w.finish(
        this,
        superclass,
        vec![field],
        vec![constructor, injector, lambda],
        class_attributes,
    )
}

#[test]
fn scans_a_jar() {
    let dir = std::env::temp_dir().join(format!("porter-jar-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let jar = dir.join("jdemo.jar");
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&jar).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let mut add = |name: &str, bytes: &[u8]| {
            zip.start_file(name, options).unwrap();
            zip.write_all(bytes).unwrap();
        };
        add(
            "fabric.mod.json",
            br#"{"id":"jdemo","version":"1.0","mixins":["jdemo.mixins.json"]}"#,
        );
        add(
            "jdemo.mixins.json",
            br#"{"package":"jdemo.mixin","mixins":["FooMixin"]}"#,
        );
        add(
            "jdemo/Gem.class",
            &class_bytes("jdemo/Gem", "net/minecraft/world/item/Item", &[], &[]),
        );
        add(
            "jdemo/Gem$Inner.class",
            &class_bytes(
                "jdemo/Gem$Inner",
                "java/lang/Object",
                &["net/minecraft/core/BlockPos"],
                &[],
            ),
        );
        add("jdemo/mixin/FooMixin.class", &mixin_class_bytes());
        add("META-INF/jars/lib.jar", b"not read");
        add("assets/jdemo/lang/en_us.json", b"{}");
        zip.finish().unwrap();
    }

    let scan = porter_java::scan(&jar).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(scan.input_kind, InputKind::Jar);
    assert_eq!(scan.metadata[0].id.as_deref(), Some("jdemo"));
    let names: Vec<&str> = scan.classes.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["jdemo.Gem", "jdemo.mixin.FooMixin"]);
    let gem = &scan.classes[0];
    assert_eq!(
        gem.superclass.as_deref(),
        Some("net.minecraft.world.item.Item")
    );
    // The nested class's references fold into its top-level class.
    assert!(
        gem.references
            .contains(&"net.minecraft.core.BlockPos".to_string())
    );
    assert!(!gem.references.iter().any(|r| r.starts_with("jdemo.Gem")));
    assert!(scan.classes[1].is_mixin);
    // Mixin details come from the annotations kept in the class file.
    let [mixin] = scan.mixins.as_slice() else {
        panic!("expected one Mixin");
    };
    assert_eq!(mixin.targets, vec!["net.minecraft.world.level.Level"]);
    let members: Vec<_> = mixin
        .members
        .iter()
        .map(|m| {
            (
                m.annotation.as_str(),
                m.verdict.name(),
                m.horizon_kind.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        members,
        vec![
            ("Shadow", "not needed", None),
            ("Inject", "supported", Some("HEAD")),
        ]
    );
    assert_eq!(mixin.members[1].target_methods, vec!["tick"]);
    assert!(mixin.members[1].cancellable);
    assert_eq!(scan.bundled_jars, vec!["META-INF/jars/lib.jar"]);
    assert_eq!(scan.resources.assets(), 1);
}

#[test]
fn reads_a_machines_menu_and_screen() {
    let scan = porter_java::scan(&fixture("neoforge-machine-demo")).unwrap();
    let menu = scan
        .menus
        .iter()
        .find(|m| m.class.ends_with("CrusherMenu"))
        .expect("the crusher's menu");
    assert_eq!(
        menu.slots
            .iter()
            .map(|s| (s.index, s.x, s.y, s.result))
            .collect::<Vec<_>>(),
        [(Some(0), 56, 35, false), (Some(1), 116, 35, true)]
    );
    assert_eq!(menu.inventory, Some([8, 84]));
    assert_eq!(menu.data_slots, 2);
    let screen = scan
        .screens
        .iter()
        .find(|s| s.class.ends_with("CrusherScreen"))
        .expect("the crusher's screen");
    assert_eq!(screen.menu.as_deref(), Some("CrusherMenu"));
    assert_eq!((screen.width, screen.height), (Some(176), Some(166)));
    assert_eq!(screen.texture.as_deref(), Some("textures/gui/crusher.png"));
}

#[test]
fn reads_network_messages_and_config() {
    let scan = porter_java::scan(&fixture("neoforge-machine-demo")).unwrap();
    let speed = scan
        .payloads
        .iter()
        .find(|p| p.class.ends_with("SetSpeed"))
        .expect("SetSpeed");
    assert_eq!(speed.channel.as_deref(), Some("grind:set_speed"));
    assert!(speed.to_server && !speed.to_client);
    assert_eq!(
        speed
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.java_type.as_str()))
            .collect::<Vec<_>>(),
        [("pos", "BlockPos"), ("speed", "int")]
    );
    let crushed = scan
        .payloads
        .iter()
        .find(|p| p.class.ends_with("Crushed"))
        .expect("Crushed");
    assert!(crushed.to_client && !crushed.to_server);

    let keys: Vec<&str> = scan.config.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(
        keys,
        [
            "crusher.sounds",
            "crusher.defaultTime",
            "crusher.noise",
            "xp"
        ]
    );
    assert_eq!(scan.config[2].values, ["SILENT", "QUIET", "LOUD"]);
}

#[test]
fn reads_registrations_through_a_generic_wrapper() {
    let scan = porter_java::scan(&fixture("neoforge-wrapper-demo")).unwrap();
    let mut got: Vec<(&str, &str)> = scan
        .registrations
        .iter()
        .filter_map(|r| Some((r.registry.as_str(), r.id.as_deref()?)))
        .collect();
    got.sort();
    got.dedup();
    assert_eq!(
        got,
        [
            ("block", "cable"),
            ("block", "crate"),
            ("block", "panel"),
            ("item", "key")
        ],
        "{:#?}",
        scan.registrations
    );
}
