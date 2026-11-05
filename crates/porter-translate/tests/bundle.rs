// Ports the fabric-client-demo fixture (Compass HUD) and checks the bundle.

use std::path::Path;

use porter_translate::bundle;

fn compass_bundle() -> bundle::Bundle {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures/fabric-client-demo");
    let scan = porter_java::scan(&path).unwrap();
    let lang = bundle::load_lang(&scan);
    bundle::generate(&scan, &lang)
}

#[test]
fn finds_the_client_features() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures/fabric-client-demo");
    let scan = porter_java::scan(&path).unwrap();
    let keys: Vec<(&str, &str)> = scan
        .client
        .keys
        .iter()
        .map(|k| (k.name.as_str(), k.default_key.as_str()))
        .collect();
    assert_eq!(
        keys,
        vec![
            ("key.compasshud.toggle", "key.keyboard.h"),
            ("key.compasshud.cycle", "key.keyboard.right.bracket"),
        ]
    );
    assert_eq!(scan.client.commands, vec!["compass"]);
}

#[test]
fn wires_the_hooks_the_mod_uses() {
    let b = compass_bundle();
    assert_eq!(b.id, "compasshud");
    let wired: Vec<&str> = b.wired.keys().copied().collect();
    assert_eq!(
        wired,
        vec![
            "command",
            "hud",
            "key",
            "mods",
            "settings",
            "settings_click",
            "tick"
        ]
    );
    let files: Vec<&str> = b.files.keys().map(String::as_str).collect();
    assert_eq!(
        files,
        vec![
            ".porter/port.json",
            "Cargo.toml",
            "PORTING.md",
            "compatibility.json",
            "horizon-native.json",
            "mixins/compasshud/click.rs",
            "mixins/compasshud/command.rs",
            "mixins/compasshud/horizon-mixin.toml",
            "mixins/compasshud/hud.rs",
            "mixins/compasshud/key.rs",
            "mixins/compasshud/list.rs",
            "mixins/compasshud/runtime.rs",
            "mixins/compasshud/settings.rs",
            "mixins/compasshud/tick.rs",
        ]
    );
}

#[test]
fn manifest_is_valid_and_complete() {
    let b = compass_bundle();
    let manifest: toml::Table =
        toml::from_str(&b.files["mixins/compasshud/horizon-mixin.toml"]).unwrap();
    assert_eq!(manifest["mod"]["id"].as_str(), Some("compasshud"));
    assert_eq!(manifest["mod"]["version"].as_str(), Some("1.2.0"));
    assert_eq!(
        manifest["module"][0]["name"].as_str(),
        Some("ported_compasshud")
    );
    let injections = manifest["injection"].as_array().unwrap();
    assert_eq!(injections.len(), 7);
    for inj in injections {
        // Every injection hooks a native hook point at HEAD, with a helper that exists.
        let target = inj["target"].as_str().unwrap();
        assert!(target.starts_with("horizon_client::game::native_hooks::Game<'_>::native_"));
        assert_eq!(inj["kind"].as_str(), Some("HEAD"));
        let helper = inj["helper"].as_str().unwrap();
        assert!(
            b.files.contains_key(&format!("mixins/compasshud/{helper}")),
            "missing helper {helper}"
        );
        assert!(!inj["bindings"].as_array().unwrap().is_empty());
    }

    let compat: serde_json::Value = serde_json::from_str(&b.files["compatibility.json"]).unwrap();
    assert_eq!(compat["game"], "26.3");
    assert_eq!(compat["hooks"], 1);
    let cargo: toml::Table = toml::from_str(&b.files["Cargo.toml"]).unwrap();
    assert_eq!(
        cargo["lib"]["path"].as_str(),
        Some("mixins/compasshud/runtime.rs")
    );
}

#[test]
fn runtime_has_keys_with_their_labels() {
    let b = compass_bundle();
    let runtime = &b.files["mixins/compasshud/runtime.rs"];
    assert!(runtime.contains(r#"("key.compasshud.toggle", "Toggle compass", "key.keyboard.h"),"#));
    assert!(runtime.contains(r#"pub const COMMANDS: &[&str] = &["compass"];"#));
    assert!(runtime.contains("pub const DESCRIPTION: &str = \"Shows which way you are facing.\";"));
    assert!(runtime.contains("TODO: port from net.compasshud.CompassHudClient."));
    for f in [
        "pub fn on_key",
        "pub fn on_command",
        "pub fn tick",
        "pub fn hud_lines",
        "pub fn settings_buttons",
    ] {
        assert!(runtime.contains(f), "{f}");
    }
    let porting = &b.files["PORTING.md"];
    assert!(porting.contains("| `native_hud` | `hud_lines` | `net.compasshud.CompassHudClient` |"));
    assert!(porting.contains("- [ ] `net.compasshud.CompassRenderer`"));
}

#[test]
fn declares_hooks_instead_of_pinning() {
    let b = compass_bundle();
    let native: serde_json::Value = serde_json::from_str(&b.files["horizon-native.json"]).unwrap();
    assert_eq!(native["schema"], 1);
    assert_eq!(native["id"], "compasshud");
    assert_eq!(native["version"], "1.2.0");
    assert_eq!(native["game"], "26.3");
    assert_eq!(native["hooks"], 1);
    assert_eq!(native["horizon"], bundle::HORIZON_REVISION);
    // Keys sorted, as horizon-mods' packager writes them.
    let keys: Vec<&str> = b.files["horizon-native.json"]
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"')?.split('"').next())
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn packages_a_deterministic_hnmod() {
    let dir = std::env::temp_dir().join(format!("porter-package-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let b = compass_bundle();
    bundle::write(&b, &dir.join("src"), false).unwrap();
    std::fs::write(dir.join("src/LICENSE"), "MIT").unwrap();

    // A client mod has no content: one .hnmod, no .hmod.
    let [first] = bundle::package(&dir.join("src"), &dir.join("out1"))
        .unwrap()
        .try_into()
        .unwrap();
    let [second] = bundle::package(&dir.join("src"), &dir.join("out2"))
        .unwrap()
        .try_into()
        .unwrap();
    assert_eq!(first.file_name().unwrap(), "compasshud-1.2.0.hnmod");
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap()
    );

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&first).unwrap()).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        vec![
            "horizon-native.json",
            "mixins/compasshud/LICENSE",
            "mixins/compasshud/click.rs",
            "mixins/compasshud/command.rs",
            "mixins/compasshud/horizon-mixin.toml",
            "mixins/compasshud/hud.rs",
            "mixins/compasshud/key.rs",
            "mixins/compasshud/list.rs",
            "mixins/compasshud/runtime.rs",
            "mixins/compasshud/settings.rs",
            "mixins/compasshud/tick.rs",
        ]
    );
    // PORTING.md, Cargo.toml and compatibility.json stay out of the .hnmod.
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn refuses_to_overwrite() {
    let dir = std::env::temp_dir().join(format!("porter-bundle-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("mine.txt"), "keep me").unwrap();
    let b = compass_bundle();
    assert!(bundle::write(&b, &dir, false).is_err());
    bundle::write(&b, &dir, true).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("mine.txt")).unwrap(),
        "keep me"
    );
    assert!(dir.join("mixins/compasshud/runtime.rs").exists());
    std::fs::remove_dir_all(&dir).ok();
}
