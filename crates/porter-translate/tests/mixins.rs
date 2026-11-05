// Ports the fabric-mixin-demo fixture (Steady View: six Mixins) and checks that
// Mixins become the Horizon hooks that do their job.

use std::path::Path;

use porter_translate::bundle;

fn steady_view() -> bundle::Bundle {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures/fabric-mixin-demo");
    let scan = porter_java::scan(&path).unwrap();
    assert_eq!(scan.mixins.len(), 6);
    let lang = bundle::load_lang(&scan);
    bundle::generate(&scan, &lang)
}

#[test]
fn mixins_become_hooks() {
    let b = steady_view();
    // Gui.render is the HUD, which the porter wires to runtime.rs.
    assert!(b.wired.contains_key("hud"));
    assert!(b.wired["hud"].contains("net.steadyview.mixin.GuiMixin"));
    // The others get a helper with Horizon's bindings to port into.
    let stubbed: Vec<&str> = b.stubbed.keys().copied().collect();
    assert_eq!(stubbed, ["can_eat", "compute_fov", "item_picked_up"]);
    assert!(
        b.stubbed["can_eat"]
            .0
            .contains("net.steadyview.mixin.PlayerMixin")
    );
    // No hook does FarmBlock.turnToDirt or LivingEntity.jumpFromGround's job.
    assert!(b.unwired.is_empty(), "{:?}", b.unwired.keys());

    // The Java's statics are shared state in runtime.rs.
    let runtime = &b.files["mixins/steadyview/runtime.rs"];
    assert!(runtime.contains("pub mod steady_view {"));
    assert!(runtime.contains(
        "pub static PICKUPS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);"
    ));
    assert!(runtime.contains("pub const FOV: f32 = 70.0;"));

    // The bundle needs the newest of the hooks it uses (can_eat came in 90).
    assert_eq!(b.hooks, 90);
    let native: serde_json::Value = serde_json::from_str(&b.files["horizon-native.json"]).unwrap();
    assert_eq!(native["hooks"], 90);
}

#[test]
fn helpers_have_horizon_bindings() {
    let b = steady_view();
    let helper = &b.files["mixins/steadyview/item_picked_up.rs"];
    assert!(helper.contains(
        "fn steadyview_item_picked_up(game: &mut crate::game::Game<'_>, player: u128, item: &str, count: u32, canceled: &mut bool)"
    ));
    assert!(helper.contains("Contract: cancel"));
    assert!(helper.contains("steadyview$countPickup on `playerTouch`"));

    let manifest: toml::Table =
        toml::from_str(&b.files["mixins/steadyview/horizon-mixin.toml"]).unwrap();
    let injections = manifest["injection"].as_array().unwrap();
    let fov = injections
        .iter()
        .find(|i| i["id"].as_str() == Some("compute-fov"))
        .unwrap();
    assert_eq!(
        fov["target"].as_str(),
        Some("horizon_client::game::native_hooks::Game<'_>::native_compute_fov")
    );
    assert_eq!(fov["helper"].as_str(), Some("compute_fov.rs"));
    let bindings: Vec<&str> = fov["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["value"].as_str().unwrap())
        .collect();
    assert_eq!(bindings, ["&*self", "partial", "camera", "&mut *degrees"]);
}

#[test]
fn porting_md_says_where_each_mixin_went() {
    let b = steady_view();
    let porting = &b.files["PORTING.md"];
    assert!(porting.contains("## Hooks to port into"));
    assert!(porting.contains(
        "| `GameRendererMixin` | `GameRenderer` | `@Inject` getFov | `native_compute_fov` |"
    ));
    assert!(porting.contains("| `FarmBlockMixin` | `FarmBlock` | `@Inject` turnToDirt | none: inject into Horizon's code |"));
    assert!(porting.contains("`native_can_eat` (helper to port into)"));
}

#[test]
fn points_unmapped_mixins_at_horizon_functions() {
    // A Horizon checkout of one file, as the launcher keeps them.
    let dir = std::env::temp_dir().join(format!("porter-ledger-{}", std::process::id()));
    let src = dir.join("crates/horizon-world/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("living_motion.rs"),
        "/// `LivingEntity.jumpFromGround`: the jump impulse.\npub fn jump_impulse() {}\n",
    )
    .unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures/fabric-mixin-demo");
    let scan = porter_java::scan(&path).unwrap();
    let b = bundle::generate_with(&scan, &bundle::load_lang(&scan), Some(&dir));
    let porting = &b.files["PORTING.md"];
    assert!(
        porting.contains(
            "none; inject into `crates/horizon-world/src/living_motion.rs:2` `jump_impulse` (ports it)"
        ),
        "{porting}"
    );
    assert!(porting.contains("| `FarmBlockMixin` | `FarmBlock` | `@Inject` turnToDirt | none, and no Horizon function names the method yet |"));
    std::fs::remove_dir_all(&dir).ok();
}
