//! Blocks whose baked model Java wraps in a class of its own that adds parts to it from the
//! world: Create's pipes draw a rim, a connector or a drain on each side and a bracket
//! (`PipeAttachmentModel`), its shafts and cogwheels a bracket (`BracketedKineticBlockModel`).
//! Horizon draws a block's model from its blockstate and has no hook for such a class, so the
//! parts it adds are drawn with the block entity, once a frame, by a function translated from
//! the class: `model_<block>(me, around, draw)`, which sees the block, its block entity's saved
//! values and the six blocks beside it.

use std::fmt::Write;

use porter_model::ModScan;

use crate::block_entity::{BlockEntity, rust_ident};

/// One block's model class.
#[derive(Clone, Debug)]
pub struct BlockModel {
    /// The block: `create:fluid_pipe`.
    pub block: String,
    /// The function that draws what the class adds: `model_fluid_pipe`.
    pub rust: String,
    /// The model class, fully qualified when found.
    pub class: String,
    /// The block entity the block has, whose drawing calls this: its id and class.
    pub entity: Option<(String, Option<String>)>,
}

/// The blocks with a model class of their own (`scan.block_models`) that have a block
/// entity, so the function can be drawn with it. Those without one are left out (their parts
/// need a hook Horizon doesn't have), and named in `without_entity`.
pub fn find(scan: &ModScan, ns: &str, entities: &[BlockEntity]) -> (Vec<BlockModel>, Vec<String>) {
    let mut out = Vec::new();
    let mut without = Vec::new();
    for (id, class) in &scan.block_models {
        let block = format!("{ns}:{id}");
        let full = scan
            .classes
            .iter()
            .filter(|c| c.name.rsplit(['.', '$']).next() == Some(class.as_str()))
            .map(|c| c.name.clone())
            .next()
            .unwrap_or_else(|| class.clone());
        let entity = entities
            .iter()
            .find(|e| e.blocks.contains(&block))
            .map(|e| (e.id.clone(), e.class.clone()));
        if entity.is_none() {
            without.push(format!("{block} ({class})"));
            continue;
        }
        out.push(BlockModel {
            rust: format!("model_{}", rust_ident(id)),
            block,
            class: full,
            entity,
        });
    }
    (out, without)
}

/// The runtime.rs code: `Around`, the table of blocks, the dispatcher and a stub a block.
pub fn runtime(models: &[BlockModel]) -> String {
    let mut r = String::from(
        r#"
/// The six blocks beside one, in `Side::ALL`'s order, each with its state and its block
/// entity's saved values (`n.get(key)`): what a model class's `gatherModelData` reads of the
/// world (`world.getBlockState(pos.relative(d))`, the block entity there).
pub struct Around {
    pub sides: Vec<Option<Neighbour>>,
}

impl Around {
    /// The block beside it on `side`.
    pub fn side(&self, side: Side) -> Option<&Neighbour> {
        let i = Side::ALL.iter().position(|s| *s == side)?;
        self.sides.get(i)?.as_ref()
    }
}

/// The blocks whose model a class of the mod's adds parts to, drawn with their block entity.
pub const BLOCK_MODELS: &[&str] = &[
"#,
    );
    for m in models {
        let _ = writeln!(r, "    {:?},", m.block);
    }
    r.push_str(
        "];\n\n/// Draws what block `block`'s model class adds to its model (nothing for another block).\npub fn draw_block_model(block: &str, me: &Neighbour, around: &Around, draw: &mut Draw) {\n    match block {\n",
    );
    for m in models {
        let _ = writeln!(r, "        {:?} => {}(me, around, draw),", m.block, m.rust);
    }
    r.push_str("        _ => {}\n    }\n}\n");
    for m in models {
        let entity = m
            .entity
            .as_ref()
            .and_then(|(_, c)| c.clone())
            .map(|c| format!(" for the block of a `{c}`, whose behaviours it reads"))
            .unwrap_or_default();
        let _ = write!(
            r,
            "
/// Draws what Java's model class for block `{block}` adds to the block's own model, once a
/// frame while it is in view. Horizon draws the block's model from its blockstate; this draws
/// the parts the class adds from the world (the quads its `getQuads` adds to
/// `super.getQuads`), from what its `gatherModelData` or `getModelData` reads. `me` is the
/// block: its state (`me.state(\"axis\")`) and its block entity's saved values
/// (`me.get(\"Bracket\")`, the keys its saved values are under). `around` holds the six blocks
/// beside it (`around.side(Side::North)`). `draw` starts at the block's lower north-west corner,
/// in blocks, unturned: the quads of a model are drawn where the model puts them. A partial
/// model (`AllPartialModels.X.get().getQuads(..)`, one of a map of them by side) is
/// `draw.model(id, 0xFFFFFF)`, its id in `STANDALONE_MODELS`; another block's model
/// (`getBlockRenderer().getBlockModel(state)`, a bracket kept as a state's text) is
/// `draw.block(block, properties)`. Draw only what the class adds, never the block's own
/// model again; where the class takes the block's own quads away or changes them (a sprite
/// shifted, connected textures, vertices moved), write a `// Missing:` line.
///
/// TODO: port the model of `{class}`{entity}.
pub fn {rust}(me: &Neighbour, around: &Around, draw: &mut Draw) {{
    let _ = (me, around, draw);
}}
",
            block = m.block,
            class = m.class,
            rust = m.rust,
        );
    }
    r
}
