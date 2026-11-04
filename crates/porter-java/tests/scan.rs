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
            "com.demo.SpeedHolder",
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

    // Registrations through DeferredRegister fields.
    let mut regs: Vec<(String, Option<String>)> = scan
        .registrations
        .iter()
        .map(|r| (r.registry.clone(), r.id.clone()))
        .collect();
    regs.sort();
    assert_eq!(
        regs,
        vec![
            ("block".to_string(), Some("crank".to_string())),
            ("block".to_string(), Some("crank_2".to_string())),
            ("block_entity_type".to_string(), Some("crank".to_string())),
            ("item".to_string(), Some("handle".to_string())),
        ]
    );

    // Resources from both resource roots.
    let r = &scan.resources;
    assert_eq!(r.assets(), 3);
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
        add(
            "jdemo/mixin/FooMixin.class",
            &class_bytes(
                "jdemo/mixin/FooMixin",
                "java/lang/Object",
                &[],
                &["Lorg/spongepowered/asm/mixin/Mixin;"],
            ),
        );
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
    assert_eq!(scan.mixins.len(), 1);
    assert!(scan.classes[1].is_mixin);
    assert_eq!(scan.bundled_jars, vec!["META-INF/jars/lib.jar"]);
    assert_eq!(scan.resources.assets(), 1);
}
