// Compiles the generated helper of every hook point on `Game` in Horizon's hook
// table, spliced into a mock of the hooked functions with the real signatures, the
// way horizon-mixins does. The mock's own types are empty stand-ins named as
// Horizon names them; what this checks is the helpers: their bindings, their
// borrows and that they compile without warnings.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::process::Command;

use porter_map::hooks;
use porter_translate::bundle;

/// Names a hook's types use that std provides.
const STD: &[&str] = &["Vec", "Option", "String", "Box", "Some", "None", "Result"];

/// The type names in `ty` the mock must declare, with whether each takes a
/// lifetime; None when a type can't be stood in for (a path, a type parameter).
fn type_names(ty: &str) -> Option<Vec<(String, bool)>> {
    if ty.contains("::") || ty.contains("dyn ") || ty.contains("impl ") {
        return None;
    }
    let mut out = Vec::new();
    let chars: Vec<char> = ty.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_uppercase() && (i == 0 || !chars[i - 1].is_ascii_alphanumeric()) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            let lifetime = chars.get(i) == Some(&'<') && chars.get(i + 1) == Some(&'\'');
            if chars.get(i) == Some(&'<') && !lifetime && !STD.contains(&name.as_str()) {
                return None; // a type parameter on one of Horizon's types
            }
            if !STD.contains(&name.as_str()) {
                out.push((name, lifetime));
            }
        } else {
            i += 1;
        }
    }
    Some(out)
}

#[test]
fn every_game_hook_helper_compiles() {
    let mut types: BTreeSet<(String, bool)> = BTreeSet::new();
    let mut fns = String::new();
    let mut tested = 0;
    for h in &hooks::catalog().hooks {
        if !h
            .target
            .starts_with("horizon_client::game::native_hooks::Game<'_>::")
        {
            continue;
        }
        let mut needed = Vec::new();
        let mut mockable = true;
        for p in h.params.iter().filter(|p| !p.contains("self")) {
            let ty = p.split_once(':').map_or("", |(_, t)| t);
            match type_names(ty) {
                Some(names) => needed.extend(names),
                None => mockable = false,
            }
        }
        if !mockable {
            continue;
        }
        let (_, source) = bundle::hook_helper("all", "ported_all", &h.name, &Default::default())
            .unwrap_or_else(|| panic!("no helper for {}", h.name));
        let bindings = h.bindings().unwrap();
        let fn_name = format!("all_{}", h.name);
        let args = bindings
            .iter()
            .map(|b| b.value.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        types.extend(needed);
        let _ = writeln!(
            fns,
            "        pub fn native_{}{}({}) {{\n{source}\n            {fn_name}({args});\n        }}",
            h.name,
            h.generics,
            h.params.join(", ")
        );
        tested += 1;
    }
    assert!(tested > 200, "only {tested} hooks could be mocked");

    let mut decls = String::new();
    for (name, lifetime) in &types {
        if *lifetime {
            let _ = writeln!(
                decls,
                "    pub struct {name}<'a>(std::marker::PhantomData<&'a ()>);"
            );
        } else {
            let _ = writeln!(decls, "    pub struct {name};");
        }
    }
    let lib = format!(
        r#"#![allow(dead_code, unused_variables, non_camel_case_types, clippy::all)]
pub mod game {{
    pub struct Game<'a> {{
        _game: std::marker::PhantomData<&'a ()>,
    }}
    pub mod ported_all {{
        pub const ID: &str = "all";
    }}
    pub mod native_hooks {{
        use super::Game;
{decls}
        impl Game<'_> {{
            pub fn native_on(&self, mod_id: &str) -> bool {{ true }}
        }}
        impl Game<'_> {{
{fns}
        }}
    }}
}}
"#
    );

    let dir = std::env::temp_dir().join(format!("porter-all-hooks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"horizon-client\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.rs"), lib).unwrap();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args(["check", "--quiet", "--offline"])
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{tested} hook helpers, compiled against the mock:\n{stderr}"
    );
    assert!(!stderr.contains("warning"), "{stderr}");
    std::fs::remove_dir_all(&dir).ok();
}
