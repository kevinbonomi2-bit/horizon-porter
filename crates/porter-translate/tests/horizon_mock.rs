// Compiles a generated bundle's helpers against a mock of Horizon's native hook API.
//
// The mock copies the real signatures (horizon-client's game/native_hooks.rs,
// mod_api.rs and extras_hud.rs, 2026-11) and splices each helper into its hook the
// way horizon-mixins does: the helper becomes a local function at the head of the
// hooked function, called with the manifest's bindings. Building Horizon itself is
// what `native.py check` does; this catches type and binding mistakes without it.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use porter_translate::bundle;

/// Hook signatures, as in horizon-client's native_hooks.rs.
const HOOKS: &[(&str, &str)] = &[
    ("native_mods", "&mut self, mods: &mut Vec<NativeMod>"),
    ("native_key", "&mut self, key: &str, pressed: bool"),
    ("native_command", "&mut self, command: &str, ran: &mut bool"),
    (
        "native_hud",
        "&mut self, batch: &mut HudBatch, corners: &mut Corners",
    ),
    ("native_tick", "&mut self"),
    ("native_wheel", "&mut self, steps: f32, claimed: &mut bool"),
    (
        "native_settings",
        "&mut self, mod_id: &str, buttons: &mut Vec<String>",
    ),
    (
        "native_settings_click",
        "&mut self, mod_id: &str, button: usize",
    ),
    (
        "native_block_entity",
        "&mut self, block: &str, entity: &mut Option<(String, Compound)>",
    ),
    (
        "native_block_entity_ticks",
        "&self, block: &str, ticks: &mut bool",
    ),
    (
        "native_block_entity_tick",
        "&mut self, pos: [i32; 3], entity: &mut ModBlockEntity",
    ),
];

const MOCK_LIB: &str = r#"
extern crate self as horizon_client;

// horizon-world's nbt module, as Horizon has it (crates/horizon-world/src/nbt.rs).
pub mod horizon_world {
    pub mod nbt {
        use std::collections::BTreeMap;
        #[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct JavaString(pub Vec<u16>);
        impl From<&str> for JavaString {
            fn from(value: &str) -> Self {
                Self(value.encode_utf16().collect())
            }
        }
        impl JavaString {
            pub fn to_string_lossy(&self) -> String {
                String::from_utf16_lossy(&self.0)
            }
        }
        pub type Compound = BTreeMap<JavaString, Tag>;
        #[derive(Clone, Debug, PartialEq)]
        pub enum Tag {
            End,
            Byte(i8),
            Short(i16),
            Int(i32),
            Long(i64),
            Float(u32),
            Double(u64),
            ByteArray(Vec<u8>),
            String(JavaString),
            List(Vec<Tag>),
            Compound(Compound),
            IntArray(Vec<i32>),
            LongArray(Vec<i64>),
        }
    }
}

// horizon-client's containers module: ModBlockEntity derefs to its fields, and
// each mutable access counts as a change.
pub mod containers {
    pub struct ItemStack;
    pub struct ModBlockEntityFields {
        pub id: String,
        pub nbt: crate::horizon_world::nbt::Compound,
        pub items: Vec<Option<ItemStack>>,
        pub data: Vec<i32>,
    }
    pub struct ModBlockEntity {
        fields: ModBlockEntityFields,
        pub changes: u64,
    }
    impl std::ops::Deref for ModBlockEntity {
        type Target = ModBlockEntityFields;
        fn deref(&self) -> &ModBlockEntityFields {
            &self.fields
        }
    }
    impl std::ops::DerefMut for ModBlockEntity {
        fn deref_mut(&mut self) -> &mut ModBlockEntityFields {
            self.changes += 1;
            &mut self.fields
        }
    }
}

pub mod hud {
    pub struct HudBatch;
}
pub mod extras {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum Corner { TopLeft, TopRight, BottomLeft, BottomRight }
}
pub mod extras_hud {
    pub struct Corners;
    impl Corners {
        pub fn place(&mut self, corner: crate::extras::Corner, w: f32, h: f32) -> (f32, f32) {
            let _ = (corner, w, h);
            (0.0, 0.0)
        }
    }
}
pub mod game {
    pub struct Game<'a> {
        _game: std::marker::PhantomData<&'a ()>,
    }
    __MODULE__
    pub mod native_hooks {
        #![allow(unused_variables, dead_code)]
        use super::Game;
        use crate::containers::ModBlockEntity;
        use crate::extras_hud::Corners;
        use crate::horizon_world::nbt::Compound;
        use crate::hud::HudBatch;
        // In Horizon, horizon_world is a dependency: helpers name it by its crate path.
        #[allow(unused_imports)]
        use crate::horizon_world;

        pub struct NativeMod {
            pub id: String,
            pub title: String,
            pub description: String,
            pub keys: Vec<[String; 3]>,
        }
        pub struct NativePlayer {
            pub x: f64,
            pub y: f64,
            pub z: f64,
            pub yaw: f32,
            pub pitch: f32,
            pub dimension: &'static str,
        }

        impl Game<'_> {
            pub fn native_on(&self, mod_id: &str) -> bool { true }
            pub fn native_playing(&self) -> bool { true }
            pub fn native_window(&self) -> bool { true }
            pub fn native_text_width(&self, text: &str) -> f32 { 0.0 }
            pub fn native_fill(&self, batch: &mut HudBatch, rect: [f32; 4], argb: u32) {}
            pub fn native_text(&self, batch: &mut HudBatch, text: &str, at: [f32; 2], rgb: u32, shadow: bool) {}
            pub fn native_player(&self) -> Option<NativePlayer> { None }
            pub fn native_mod_key(&self, mod_id: &str, name: &str, default: &str) -> String { String::new() }
            pub fn native_chat(&mut self, text: &str) {}
            pub fn native_setting(&self, mod_id: &str, key: &str) -> Option<String> { None }
            pub fn native_set_setting(&self, mod_id: &str, key: &str, value: &str) {}
        }

        impl Game<'_> {
__HOOKS__
        }
    }
}
"#;

fn compass_scan() -> porter_model::ModScan {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures/fabric-client-demo");
    porter_java::scan(&path).unwrap()
}

#[test]
fn helpers_compile_against_horizon_api() {
    let mut scan = compass_scan();
    // The fixture uses every wired hook but the wheel: add a wheel listener so the
    // check covers all of them.
    scan.classes[0]
        .references
        .push("net.neoforged.neoforge.client.event.InputEvent.MouseScrollingEvent".into());
    let lang = bundle::load_lang(&scan);
    let b = bundle::generate(&scan, &lang);
    assert_eq!(
        b.wired.len(),
        porter_map::WIRED_HOOKS.len(),
        "{:?}",
        b.wired.keys()
    );
    compiles_against_the_mock(&b, "client");
}

#[test]
fn block_entity_helpers_compile_against_horizon_api() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../porter-java/tests/fixtures/neoforge-demo");
    let scan = porter_java::scan(&path).unwrap();
    let lang = bundle::load_lang(&scan);
    let b = bundle::generate(&scan, &lang);
    for hook in ["block_entity", "block_entity_ticks", "block_entity_tick"] {
        assert!(b.wired.contains_key(hook), "{hook}");
    }
    compiles_against_the_mock(&b, "blocks");
}

/// Splices the bundle's helpers into the mock's hooks and runs `cargo check` on it.
fn compiles_against_the_mock(b: &bundle::Bundle, name: &str) {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("porter-horizon-mock-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle_dir = dir.join("bundle");
    bundle::write(b, &bundle_dir, false).unwrap();
    let mixins = bundle_dir.join("mixins").join(&b.id);

    let manifest: toml::Table =
        toml::from_str(&b.files[&format!("mixins/{}/horizon-mixin.toml", b.id)]).unwrap();
    let module = manifest["module"][0]["name"].as_str().unwrap();

    let mut hooks = String::new();
    for (name, params) in HOOKS {
        let injection = manifest["injection"].as_array().unwrap().iter().find(|i| {
            i["target"]
                .as_str()
                .unwrap()
                .ends_with(&format!("::{name}"))
        });
        let _ = writeln!(hooks, "            pub fn {name}({params}) {{");
        if let Some(inj) = injection {
            let helper = mixins.join(inj["helper"].as_str().unwrap());
            let source = std::fs::read_to_string(&helper).unwrap();
            let fn_name = source
                .split("fn ")
                .nth(1)
                .and_then(|s| s.split('(').next())
                .unwrap()
                .trim()
                .to_string();
            let args: Vec<&str> = inj["bindings"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b["value"].as_str().unwrap())
                .collect();
            // Pasted in, as horizon-mixins does (include! cannot add items here).
            let _ = writeln!(
                hooks,
                "{source}\n                {fn_name}({});",
                args.join(", ")
            );
        }
        hooks.push_str("            }\n");
    }

    let runtime = mixins.join("runtime.rs");
    let lib = MOCK_LIB
        .replace(
            "__MODULE__",
            &format!(
                "#[path = {:?}]\n    pub mod {module};",
                runtime.to_string_lossy()
            ),
        )
        .replace("__HOOKS__", &hooks);
    let mock = dir.join("mock");
    std::fs::create_dir_all(mock.join("src")).unwrap();
    std::fs::write(
        mock.join("Cargo.toml"),
        "[package]\nname = \"horizon-client\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(mock.join("src/lib.rs"), lib).unwrap();

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args(["check", "--quiet", "--offline"])
        .current_dir(&mock)
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the generated helpers do not compile against the Horizon API mock:\n{stderr}"
    );
    // Warnings in generated code count as failures too.
    assert!(!stderr.contains("warning"), "{stderr}");
    std::fs::remove_dir_all(&dir).ok();
}
