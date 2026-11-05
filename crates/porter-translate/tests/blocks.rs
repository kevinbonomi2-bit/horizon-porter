// Ports the neoforge-demo fixture (a crank block with a ticking block entity) and
// checks the content mod and the block entity code.

use std::path::Path;

use porter_translate::block_entity::Kind;
use porter_translate::bundle;

fn demo_bundle() -> bundle::Bundle {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../porter-java/tests/fixtures/neoforge-demo");
    let scan = porter_java::scan(&path).unwrap();
    let lang = bundle::load_lang(&scan);
    bundle::generate(&scan, &lang)
}

#[test]
fn writes_a_content_mod() {
    let b = demo_bundle();
    assert!(b.native, "the block entity needs the native half");
    assert_eq!(b.hooks, porter_translate::block_entity::HOOKS_VERSION);

    let manifest: toml::Table = toml::from_str(&b.files["content/horizon-mod.toml"]).unwrap();
    assert_eq!(manifest["id"].as_str(), Some("demo"));
    assert_eq!(manifest["api"].as_str(), Some("0.2"));
    assert_eq!(manifest["side"].as_str(), Some("both"));
    assert_eq!(
        manifest["capabilities"].as_array().unwrap()[0].as_str(),
        Some("registries")
    );
    // Only the keys Horizon's manifest allows.
    for key in manifest.keys() {
        assert!(
            [
                "id",
                "name",
                "version",
                "description",
                "authors",
                "api",
                "side",
                "capabilities",
                "namespaces"
            ]
            .contains(&key.as_str()),
            "{key}"
        );
    }

    // The crank copies the iron block it names, with what its properties change.
    let crank: serde_json::Value =
        serde_json::from_str(&b.files["content/data/demo/horizon/block/crank.json"]).unwrap();
    assert_eq!(crank["template"], "minecraft:iron_block");
    assert_eq!(crank["properties"]["hardness"], 3.5);
    assert_eq!(crank["properties"]["explosion_resistance"], 6.0);
    assert_eq!(crank["properties"]["light"], 7);
    assert_eq!(crank["properties"]["sounds"], "minecraft:copper_block");
    // crank_2 names no vanilla block: its class extends Block, so stone.
    let crank2: serde_json::Value =
        serde_json::from_str(&b.files["content/data/demo/horizon/block/crank_2.json"]).unwrap();
    assert_eq!(crank2["template"], "minecraft:stone");

    // Items that are not a block's.
    assert!(
        b.files
            .contains_key("content/data/demo/horizon/item/handle.json")
    );
    assert!(
        !b.files
            .contains_key("content/data/demo/horizon/item/crank.json")
    );

    // The Java mod's own resources, copied.
    let copies: Vec<&str> = b.copies.keys().map(String::as_str).collect();
    for f in [
        "content/assets/demo/blockstates/crank.json",
        "content/assets/demo/lang/en_us.json",
        "content/assets/demo/models/block/crank.json",
        "content/data/demo/recipe/crank.json",
        "content/data/minecraft/tags/block/mineable/pickaxe.json",
    ] {
        assert!(copies.contains(&f), "{f} not in {copies:?}");
    }
}

#[test]
fn turns_the_block_entity_into_state() {
    let b = demo_bundle();
    let [be] = b.block_entities.as_slice() else {
        panic!("{:?}", b.block_entities);
    };
    assert_eq!(be.id, "demo:crank");
    assert_eq!(be.blocks, vec!["demo:crank"]);
    assert_eq!(be.class.as_deref(), Some("com.demo.block.CrankBlockEntity"));
    assert_eq!(be.rust, "Crank");
    assert!(be.ticks);
    let fields: Vec<(&str, Kind, &str, bool)> = be
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.kind, f.key.as_str(), f.saved_by_java))
        .collect();
    assert_eq!(
        fields,
        vec![
            ("speed", Kind::Float, "Speed", true),
            ("turns", Kind::Int, "Turns", true),
            ("powered", Kind::Bool, "Powered", true),
            ("owner", Kind::Text, "Owner", true),
        ]
    );
    assert!(be.left_out.iter().any(|l| l.contains("MAX_SPEED")));

    let wired: Vec<&str> = b.wired.keys().copied().collect();
    for hook in ["block_entity", "block_entity_tick", "block_entity_ticks"] {
        assert!(wired.contains(&hook), "{hook} not in {wired:?}");
    }
    let runtime = &b.files["mixins/demo/runtime.rs"];
    assert!(runtime.contains(r#"("demo:crank", "demo:crank", true),"#));
    assert!(runtime.contains("pub fn tick_crank(pos: [i32; 3], state: &mut Crank)"));
    assert!(runtime.contains(r#"("Speed", Saved::Float(self.speed)),"#));
    assert!(runtime.contains(r#"("Powered", Saved::Byte(i8::from(self.powered))),"#));
    let porting = &b.files["PORTING.md"];
    assert!(porting.contains("## Block entities"));
    assert!(porting.contains("| `crank` | `minecraft:iron_block` |"));
}

#[test]
fn generated_runtime_passes_its_own_tests() {
    // The bundle is a Cargo package: `cargo test` in it runs runtime.rs's tests,
    // including the save-and-load round trip of each block entity state.
    let dir = std::env::temp_dir().join(format!("porter-blocks-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let b = demo_bundle();
    bundle::write(&b, &dir, false).unwrap();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = std::process::Command::new(cargo)
        .args(["test", "--quiet", "--offline"])
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{text}");
    assert!(!text.contains("warning"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn packages_the_content_mod() {
    let dir = std::env::temp_dir().join(format!("porter-blocks-pack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let b = demo_bundle();
    bundle::write(&b, &dir.join("src"), false).unwrap();
    let files = bundle::package(&dir.join("src"), &dir.join("out")).unwrap();
    let names: Vec<String> = files
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["demo-0.3.0.hnmod", "demo-0.3.0.hmod"]);
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&files[1]).unwrap()).unwrap();
    let entries: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().unwrap().to_string())
        .collect();
    assert!(entries.contains(&"horizon-mod.toml".to_string()));
    assert!(entries.contains(&"data/demo/horizon/block/crank.json".to_string()));
    assert!(entries.contains(&"assets/demo/lang/en_us.json".to_string()));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ports_a_machine() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures/neoforge-machine-demo");
    let scan = porter_java::scan(&path).unwrap();
    let b = bundle::generate(&scan, &bundle::load_lang(&scan));
    let m = &b.machines[0];
    assert_eq!(
        (
            m.entity.as_str(),
            m.menu_type.as_str(),
            m.slots,
            m.data_slots
        ),
        ("grind:crusher", "grind:crusher", 2, 2)
    );
    assert_eq!(m.results, [1]);
    assert_eq!(m.layout, [Some([56, 35]), Some([116, 35])]);
    assert_eq!(m.texture.as_deref(), Some("grind:gui/crusher"));

    let menu: serde_json::Value =
        serde_json::from_str(&b.files["content/data/grind/horizon/menu/crusher.json"]).unwrap();
    assert_eq!(
        menu,
        serde_json::json!({ "slots": 2, "data_slots": 2, "results": [1] })
    );
    let block: serde_json::Value =
        serde_json::from_str(&b.files["content/data/grind/horizon/block/crusher.json"]).unwrap();
    assert_eq!(block["template"], "minecraft:furnace");

    let runtime = &b.files["mixins/grind/runtime.rs"];
    assert!(runtime.contains(
        "pub fn tick_crusher(pos: [i32; 3], state: &mut Crusher, machine: &mut Machine<'_>)"
    ));
    assert!(runtime.contains(r#"pub const RECIPE_TYPES: &[&str] = &["grind:crushing"];"#));
    assert!(runtime.contains("pub fn draw_crusher(data: &[i32]) -> Vec<Sprite>"));
    assert!(b.hooks >= 85);
    assert!(b.files["PORTING.md"].contains("## Machines"));
}

#[test]
fn ports_network_messages_and_config() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures/neoforge-machine-demo");
    let scan = porter_java::scan(&path).unwrap();
    let b = bundle::generate(&scan, &bundle::load_lang(&scan));
    for hook in [
        "net_from_client",
        "net_from_server",
        "settings",
        "settings_click",
    ] {
        assert!(b.wired.contains_key(hook), "{hook}");
    }
    let runtime = &b.files["mixins/grind/runtime.rs"];
    assert!(
        runtime.contains("pub struct SetSpeed {\n    pub pos: [i32; 3],\n    pub speed: i32,\n}")
    );
    assert!(runtime.contains("const CHANNEL: &'static str = \"grind:set_speed\";"));
    assert!(runtime.contains(
        "pub fn on_set_speed_from_client(player: u128, msg: SetSpeed, out: &mut Outbox)"
    ));
    assert!(runtime.contains("pub fn on_crushed_from_server(msg: Crushed, out: &mut Outbox)"));
    assert!(runtime.contains("pub crusher_default_time: i32,"));
    assert!(runtime.contains("format!(\"Default time: {}\", c.crusher_default_time)"));
    assert!(!runtime.contains("TODO: port the config screen"));
    // Horizon's networking came in hook points version 40.
    assert!(b.hooks >= 40);
    let porting = &b.files["PORTING.md"];
    assert!(porting.contains("## Network messages") && porting.contains("## Config"));
}
