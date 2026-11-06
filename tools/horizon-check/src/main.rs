//! Loads content mods with Horizon's own code, as the game does at start, and says
//! what would keep them from loading or drawing:
//!
//! 1. the content build (`mod_content::build`): a file or block Horizon refuses
//!    keeps every content mod from loading;
//! 2. the world's rules over the mods' blocks (worldgen data and live features):
//!    a world won't open when they fail ("unknown growing plant family");
//! 3. every mod block's models, baked as the game bakes them: a state no
//!    blockstate variant covers draws nothing.
//!
//! `horizon-check <facts folder> <out folder> <mod>...` (a `.hmod` or a content
//! folder). Built against the Horizon source the launcher has by
//! `tools/horizon_check.py`, which writes this crate's `Cargo.toml`.
//! Exits 1 when a stage fails or a state has no model.

use horizon_client::mod_content::{ContentMod, build};
use horizon_mods::{content::Content, manifest::Package};
use horizon_world::registry::{BlockRegistry, BlockStateId};
use std::collections::BTreeMap;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: horizon-check <facts folder> <out folder> <mod>...");
        std::process::exit(2);
    }
    let (base, out) = (Path::new(&args[1]), Path::new(&args[2]));
    let mut mods = Vec::new();
    for dir in &args[3..] {
        let dir = Path::new(dir);
        match Package::read(dir)
            .and_then(|p| Content::read(dir, &p.manifest).map(|c| (p.manifest.id, c)))
        {
            Ok((id, content)) => mods.push(ContentMod { id, content }),
            Err(e) => fail(&format!("CONTENT REFUSED {}: {e}", dir.display())),
        }
    }
    let _ = std::fs::remove_dir_all(out);
    match build(base, &mods, out) {
        Ok(notes) => {
            for n in notes {
                println!("note: {n}");
            }
            println!("content: ok ({} mods)", mods.len());
        }
        Err(e) => fail(&format!("CONTENT NOT LOADED: {e}")),
    }
    let facts = std::fs::read(out.join("facts.json")).expect("facts.json");
    let registry: &'static BlockRegistry = Box::leak(Box::new(
        BlockRegistry::from_facts_json(&facts[..]).expect("block registry"),
    ));
    match horizon_client::vanillagen::shared_data(&out.join("worldgen"), registry)
        .map_err(|e| e.to_string())
        .and_then(|d| {
            horizon_world::worldgen::live::LiveFeatures::new(&d).map_err(|e| e.to_string())
        }) {
        Ok(_) => println!("world: ok"),
        Err(e) => fail(&format!("WORLD WON'T OPEN: {e}")),
    }
    if models(out, registry) > 0 {
        std::process::exit(1);
    }
}

fn fail(line: &str) -> ! {
    println!("{line}");
    std::process::exit(1);
}

/// Bakes every mod block's models; returns how many blocks have a state no
/// variant covers. States whose models have no faces are listed too, as notes:
/// many draw nothing in Java as well (an open garage door's middle).
fn models(out: &Path, registry: &'static BlockRegistry) -> usize {
    let facts: BTreeMap<String, serde_json::Value> = serde_json::from_slice(
        &std::fs::read(out.join("model-states.json")).expect("model-states.json"),
    )
    .expect("model states");
    let blocks: Vec<&str> = facts
        .keys()
        .filter(|n| !n.starts_with("minecraft:") && registry.default_state(n).is_some())
        .map(String::as_str)
        .collect();
    let mut sprites = horizon_client_render::sprites::Sprites::default();
    let catalog = match horizon_client_render::models::catalog::StateModels::from_facts(
        out,
        registry,
        &blocks,
        &mut sprites,
        &facts,
    ) {
        Ok(c) => c,
        Err(e) => fail(&format!("MODELS NOT LOADED: {e}")),
    };
    let mut by_block: BTreeMap<&str, Vec<BlockStateId>> = BTreeMap::new();
    for i in 0..registry.len() {
        let id = BlockStateId(i as u32);
        if let Some(state) = registry.state(id)
            && let Some(name) = blocks.iter().find(|b| **b == state.name)
        {
            by_block.entry(name).or_default().push(id);
        }
    }
    let mut unbound_blocks = 0;
    for (name, ids) in &by_block {
        let (mut unbound, mut empty) = (Vec::new(), 0);
        for &id in ids {
            match catalog.collect(id, horizon_core::position::BlockPos::default()) {
                Err(_) => unbound.push(id),
                Ok(m) if m.iter().all(|(_, model)| model.faces.is_empty()) => empty += 1,
                Ok(_) => {}
            }
        }
        if let Some(&first) = unbound.first() {
            unbound_blocks += 1;
            let state = registry.state(first).expect("state");
            println!(
                "NO MODEL {name}: {} of {} states (e.g. {:?})",
                unbound.len(),
                ids.len(),
                state.properties
            );
        }
        if empty > 0 {
            println!("note: {name}: {empty} of {} states draw no faces", ids.len());
        }
    }
    println!(
        "models: {} blocks, {unbound_blocks} with states no model covers",
        blocks.len()
    );
    unbound_blocks
}
