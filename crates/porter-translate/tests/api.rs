//! The port as a trait mod on Horizon's mod API (`api/`).

use std::path::Path;

use porter_translate::bundle;

fn shutters() -> bundle::Bundle {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures/neoforge-shutter-demo");
    let scan = porter_java::scan(&path).unwrap();
    let lang = bundle::load_lang(&scan);
    bundle::generate(&scan, &lang)
}

#[test]
fn writes_a_trait_mod_beside_the_bundle() {
    let b = shutters();
    let lib = &b.api["api/src/lib.rs"];
    // A part method for each kind the shutters' Java has.
    for method in [
        "fn state_for_placement(",
        "fn update_shape(",
        "fn use_item_on(",
        "fn use_without_item(",
        "fn random_tick(",
        "fn tick_block(",
        "fn can_survive(",
        "fn analog_output(",
        "fn use_on(",
        "fn use_item(",
        "fn tooltip(",
    ] {
        assert!(lib.contains(method), "{method}");
    }
    assert!(lib.contains("\"shutters:oak_shutter\""));
    assert!(lib.contains("\"shutters:wrench\""));
    let manifest = &b.api["api/horizon-mod.toml"];
    assert!(manifest.contains("[native]"), "{manifest}");
    assert!(manifest.contains(porter_translate::api::API_HORIZON));
    assert!(b.api["api/Cargo.toml"].contains("horizon-mod-api"));

    // Written only when asked, with the content and the bundle's runtime.rs.
    let dir = std::env::temp_dir().join(format!("porter-api-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    bundle::write(&b, &dir, false).unwrap();
    assert!(!dir.join("api").exists());
    let mut b = b;
    b.write_api = true;
    bundle::write(&b, &dir, true).unwrap();
    let runtime = std::fs::read_to_string(dir.join("api/src/runtime.rs")).unwrap();
    assert!(runtime.contains("pub fn use_block("));
    assert!(dir.join("api/horizon-mod.toml").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Compiles the shutters' trait mod against the mod API at `HORIZON_MOD_API` (a
/// checkout's `crates/horizon-mod-api`, at `api::API_HORIZON`): skipped without it.
#[test]
fn compiles_against_the_mod_api() {
    let Some(api) = std::env::var_os("HORIZON_MOD_API") else {
        eprintln!("HORIZON_MOD_API not set: skipped");
        return;
    };
    let mut b = shutters();
    b.write_api = true;
    let dir = std::env::temp_dir().join(format!("porter-api-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    bundle::write(&b, &dir, false).unwrap();
    let crate_dir = dir.join("api");
    let cargo = std::fs::read_to_string(crate_dir.join("Cargo.toml")).unwrap();
    let start = cargo.find("horizon-mod-api = ").unwrap();
    let end = start + cargo[start..].find('\n').unwrap();
    let local = format!(
        "horizon-mod-api = {{ path = {:?} }}",
        Path::new(&api).to_string_lossy()
    );
    let cargo = format!("{}{}{}\n[workspace]\n", &cargo[..start], local, &cargo[end..]);
    std::fs::write(crate_dir.join("Cargo.toml"), cargo).unwrap();
    let out = std::process::Command::new("cargo")
        .args(["check", "--message-format", "short"])
        .current_dir(&crate_dir)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{text}");
    assert!(!text.contains("warning:"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
