//! A mod's machine GUIs, read from its source: each container menu's slots
//! (`addSlot(new Slot(container, index, x, y))`), which of them only give items out,
//! where the player's inventory sits, and how many data slots it syncs; and each
//! container screen's size, texture and menu.
//!
//! It reads the source text: the shapes mods write these in are few and regular.

use porter_model::{
    BlockShapeInfo, MenuInfo, MenuSlot, MobInfo, RendererInfo, ScreenInfo, StorageInfo,
};

/// The menu a class's source declares (`class` is its fully qualified name).
pub fn menu(class: &str, source: &str) -> MenuInfo {
    let mut info = MenuInfo {
        class: class.to_string(),
        ..Default::default()
    };
    let mut inventory: Option<[i32; 2]> = None;
    for args in calls(source, "addSlot") {
        let Some(new) = args.trim().strip_prefix("new ") else {
            continue;
        };
        let Some(open) = new.find('(') else {
            continue;
        };
        let ty = new[..open].trim();
        let Some(inner) = balanced(&new[open..]) else {
            continue;
        };
        let parts = split_top(inner);
        if parts.len() < 4 {
            continue;
        }
        let n = parts.len();
        let (container, index, x, y) = (&parts[0], &parts[n - 3], &parts[n - 2], &parts[n - 1]);
        // The player's own inventory: only where it starts matters.
        if is_player_inventory(container) {
            if let (Some(x), Some(y)) = (first_int(x), first_int(y)) {
                inventory = Some(match inventory {
                    Some([ix, iy]) if iy <= y => [ix.min(x), iy],
                    _ => [x, y],
                });
            }
            continue;
        }
        let (Some(sx), Some(sy)) = (int(x), int(y)) else {
            // `8 + i * 18` in a loop: a row of slots the scanner can't count.
            if let (Some(sx), Some(sy)) = (first_int(x), first_int(y)) {
                info.slots.push(MenuSlot {
                    index: int(index),
                    x: sx,
                    y: sy,
                    result: is_result(ty, source),
                });
            }
            info.approximate = true;
            continue;
        };
        info.slots.push(MenuSlot {
            index: int(index),
            x: sx,
            y: sy,
            result: is_result(ty, source),
        });
    }
    info.inventory = inventory;
    if !calls(source, "addDataSlots").is_empty() {
        info.data_slots = calls(source, "SimpleContainerData")
            .iter()
            .filter_map(|a| int(a))
            .next()
            .unwrap_or(0) as usize;
    }
    info
}

/// The screen a class's source declares.
pub fn screen(class: &str, source: &str) -> ScreenInfo {
    let menu = ["AbstractContainerScreen<", "HandledScreen<"]
        .iter()
        .find_map(|p| {
            let at = source.find(p)? + p.len();
            let name: String = source[at..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            (!name.is_empty()).then(|| name.rsplit('.').next().unwrap_or(&name).to_string())
        });
    let assigned = |names: &[&str]| {
        names.iter().find_map(|name| {
            let mut from = 0;
            while let Some(i) = source[from..].find(name).map(|i| from + i) {
                let rest = source[i + name.len()..].trim_start();
                if let Some(value) = rest.strip_prefix('=')
                    && !value.starts_with('=')
                {
                    let value: String = value
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    if let Ok(v) = value.parse() {
                        return Some(v);
                    }
                }
                from = i + name.len();
            }
            None
        })
    };
    let texture = source.split('"').skip(1).step_by(2).find_map(|s| {
        let path = s.split(':').next_back().unwrap_or(s);
        (path.starts_with("textures/gui/") && path.ends_with(".png")).then(|| path.to_string())
    });
    ScreenInfo {
        class: class.to_string(),
        menu,
        width: assigned(&["imageWidth", "backgroundWidth"]),
        height: assigned(&["imageHeight", "backgroundHeight"]),
        texture,
    }
}

/// Whether a slot's container argument is the player's inventory.
fn is_player_inventory(arg: &str) -> bool {
    let a = arg.to_ascii_lowercase();
    (a.contains("inv") && !a.contains("handler") && !a.contains("items")) || a.contains("player")
}

/// Whether a slot of type `ty` only gives items out: a result or output slot, or
/// one of the file's own slot classes whose `mayPlace` answers false.
fn is_result(ty: &str, source: &str) -> bool {
    let simple = ty.rsplit('.').next().unwrap_or(ty);
    if ["Result", "Output", "Furnace"]
        .iter()
        .any(|w| simple.contains(w))
    {
        return true;
    }
    let Some(at) = source.find(&format!("class {simple} ")) else {
        return false;
    };
    let body = &source[at..];
    let body = body
        .find('{')
        .and_then(|b| balanced_braces(&body[b..]))
        .unwrap_or("");
    body.find("mayPlace").is_some_and(|m| {
        let after = &body[m..];
        let end = after.find('}').unwrap_or(after.len());
        after[..end].contains("return false")
    })
}

/// A mob effect class's category and color, from its constructor's
/// `super(MobEffectCategory.HARMFUL, 0x8E3B46)`.
pub fn effect(source: &str) -> Option<(String, u32)> {
    if !source.contains("MobEffectCategory.") {
        return None;
    }
    calls(source, "super").iter().find_map(|args| {
        let parts = split_top(args);
        let category = parts.first()?.trim().strip_prefix("MobEffectCategory.")?;
        if !["BENEFICIAL", "HARMFUL", "NEUTRAL"].contains(&category) {
            return None;
        }
        Some((category.to_ascii_lowercase(), color(parts.get(1)?)?))
    })
}

/// An RGB color as code writes one: `0x8E3B46`, `9321286`, `-12345` (ARGB).
pub fn color(text: &str) -> Option<u32> {
    let t = text.trim();
    let v = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        i64::from_str_radix(h, 16).ok()?
    } else {
        t.parse::<i64>().ok()?
    };
    Some((v as u32) & 0xFF_FFFF)
}

/// A mob class's default attributes, from its `createAttributes()` chain:
/// `.add(Attributes.MAX_HEALTH, 40.0D)`.
pub fn mob(class: &str, source: &str) -> Option<MobInfo> {
    if !source.contains("Attributes.") {
        return None;
    }
    let mut attributes = Vec::new();
    for args in calls(source, "add") {
        let parts = split_top(&args);
        let [attr, value] = &parts[..] else {
            continue;
        };
        let Some(name) = attr.trim().strip_prefix("Attributes.") else {
            continue;
        };
        if !name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            continue;
        }
        let Ok(v) = value
            .trim()
            .trim_end_matches(['D', 'd', 'F', 'f'])
            .parse::<f64>()
        else {
            continue;
        };
        let id = format!("minecraft:{}", name.to_ascii_lowercase());
        if !attributes.iter().any(|(a, _)| *a == id) {
            attributes.push((id, v));
        }
    }
    (!attributes.is_empty()).then(|| MobInfo {
        class: class.to_string(),
        attributes,
    })
}

/// A block class that isn't a full cube: one that defines its own shape
/// (`getShape` with a `VoxelShape`), or a `PipeBlock` with its width.
pub fn block_shape(class: &str, superclass: Option<&str>, source: &str) -> Option<BlockShapeInfo> {
    let simple = superclass.map(|s| s.rsplit('.').next().unwrap_or(s));
    let pipe_thickness = (simple == Some("PipeBlock")).then(|| {
        calls(source, "super")
            .iter()
            .find_map(|args| {
                let first = split_top(args).into_iter().next()?;
                first
                    .trim()
                    .trim_end_matches(['F', 'f', 'D', 'd'])
                    .parse::<f32>()
                    .ok()
            })
            // Older mappings give the half width in blocks (0.3125F).
            .map(|v| if v < 1.0 { v * 32.0 } else { v })
            .unwrap_or(10.0)
    });
    let custom_shape = source.contains("VoxelShape")
        && (source.contains("getShape(") || source.contains("getCollisionShape("));
    (custom_shape || pipe_thickness.is_some()).then(|| BlockShapeInfo {
        class: class.to_string(),
        custom_shape,
        pipe_thickness,
    })
}

/// A class's fluid tanks and energy buffer, from the handlers it builds:
/// NeoForge's `FluidStacksResourceHandler(tanks, capacity)` and
/// `SimpleEnergyHandler(capacity, maxInsert, maxExtract)`, the older `FluidTank`
/// and `EnergyStorage`, and Team Reborn's `SimpleEnergyStorage` (Fabric).
pub fn storage(class: &str, source: &str) -> Option<StorageInfo> {
    let ints = |args: &str| -> Vec<i32> {
        split_top(args)
            .iter()
            .map(|a| constant(a, source))
            .collect::<Option<Vec<i32>>>()
            .unwrap_or_default()
    };
    let mut tanks = Vec::new();
    for args in calls(source, "new FluidTank") {
        if let Some(cap) = ints(&args).first() {
            tanks.push(*cap);
        }
    }
    for args in calls(source, "new FluidStacksResourceHandler") {
        if let [n, cap, ..] = ints(&args)[..] {
            tanks.extend(std::iter::repeat_n(cap, n.clamp(0, 16) as usize));
        }
    }
    let mut energy = None;
    for name in [
        "new SimpleEnergyHandler",
        "new EnergyStorage",
        "new SimpleEnergyStorage",
    ] {
        for args in calls(source, name) {
            let v = ints(&args);
            energy = match v[..] {
                [cap] => Some([cap, cap, cap]),
                [cap, io] => Some([cap, io, io]),
                [cap, i, o, ..] => Some([cap, i, o]),
                _ => energy,
            };
        }
    }
    (!tanks.is_empty() || energy.is_some()).then(|| StorageInfo {
        class: class.to_string(),
        tanks,
        energy,
    })
}

/// An int literal (`4_000`, `4000`), or a `static final int` of `source` it names.
fn constant(text: &str, source: &str) -> Option<i32> {
    let t = text.trim();
    if let Ok(v) = t.replace('_', "").parse() {
        return Some(v);
    }
    // `FluidType.BUCKET_VOLUME * 4`, `CAPACITY`.
    if let Some((a, b)) = t.split_once('*') {
        return constant(a, source)?.checked_mul(constant(b, source)?);
    }
    if t == "FluidType.BUCKET_VOLUME" || t == "BUCKET_VOLUME" {
        return Some(1000);
    }
    let name = t.rsplit('.').next()?;
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let decl = format!(" {name} =");
    let i = source.find(&decl)?;
    let value = source[i + decl.len()..].split(';').next()?;
    if value.contains(name) {
        return None;
    }
    constant(value, source)
}

/// A block entity renderer: the block entity class in its
/// `BlockEntityRenderer<T>` (or `<T, S>`, with a render state) or
/// `BlockEntityRendererProvider<T>`.
pub fn renderer(class: &str, source: &str) -> Option<RendererInfo> {
    for marker in ["BlockEntityRenderer<", "BlockEntityRendererProvider<"] {
        let mut from = 0;
        while let Some(i) = source[from..].find(marker).map(|i| from + i) {
            from = i + marker.len();
            let before = &source[..i];
            // Only the class's own `implements`, not a field or a parameter.
            let head = before.rsplit(['{', ';', '}']).next().unwrap_or(before);
            if !head.contains("implements") {
                continue;
            }
            let args: String = source[from..]
                .chars()
                .scan(1i32, |depth, c| {
                    match c {
                        '<' => *depth += 1,
                        '>' => *depth -= 1,
                        _ => {}
                    }
                    (*depth > 0).then_some(c)
                })
                .collect();
            let entity = split_top(&args).into_iter().next()?;
            let entity = entity
                .rsplit('.')
                .next()
                .unwrap_or(&entity)
                .trim()
                .to_string();
            if entity.is_empty() || entity.contains('?') {
                return None;
            }
            return Some(RendererInfo {
                class: class.to_string(),
                entity,
            });
        }
    }
    None
}

/// A block class's state definition: the property fields its
/// `createBlockStateDefinition` adds (`builder.add(FACING, OPEN)`), and the values its
/// constructor's `registerDefaultState` sets, as (field, value) with the value as
/// a blockstate names it (`Direction.NORTH` is `north`, `GaragePart.TOP` `top`).
pub(crate) fn state_definition(source: &str) -> Option<(Vec<String>, Vec<(String, String)>)> {
    let constant = |t: &str| {
        let w = t
            .trim()
            .rsplit(['.', ' ', '(', ')'])
            .next()
            .unwrap_or("")
            .to_string();
        (!w.is_empty()
            && w.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
        .then_some(w)
    };
    let mut fields = Vec::new();
    if let Some(i) = source.find("createBlockStateDefinition(")
        && let Some(open) = source[i..].find('{')
        && let Some(body) = balanced_braces(&source[i + open..])
    {
        for args in calls(body, "add") {
            let inner = args.trim();
            let inner = inner
                .find('{')
                .map_or(inner, |b| inner[b + 1..].trim_end_matches('}'));
            for a in split_top(inner) {
                if let Some(f) = constant(&a)
                    && !fields.contains(&f)
                {
                    fields.push(f);
                }
            }
        }
    }
    let mut defaults = Vec::new();
    for args in calls(source, "registerDefaultState") {
        for set in calls(&args, "setValue") {
            let parts = split_top(&set);
            let [field, value] = parts.as_slice() else {
                continue;
            };
            let (Some(field), Some(value)) = (constant(field), state_value(value)) else {
                continue;
            };
            if !defaults.iter().any(|(f, _): &(String, String)| *f == field) {
                defaults.push((field, value));
            }
        }
    }
    (!fields.is_empty() || !defaults.is_empty()).then_some((fields, defaults))
}

/// A state value as Java writes it, as a blockstate names it: `true`,
/// `Boolean.FALSE` (`false`), `3`, `Direction.NORTH` (`north`), `Part.TOP` (`top`).
fn state_value(text: &str) -> Option<String> {
    let t = text.trim();
    let t = t.strip_prefix("(Boolean)").unwrap_or(t).trim();
    let t = t
        .strip_prefix("Integer.valueOf(")
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or(t)
        .trim();
    if t.chars().all(|c| c.is_ascii_digit()) && !t.is_empty() {
        return Some(t.to_string());
    }
    let last = t.rsplit('.').next()?.trim();
    if last.is_empty() || !last.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(last.to_ascii_lowercase())
}

/// How a block class turns its `facing` from the player's look when placed, in
/// quarter turns clockwise: its `getStateForPlacement` sets `FACING` to
/// `context.getHorizontalDirection()` (0, away from the player), with
/// `.getClockWise()` (1), `.getOpposite()` (2) or `.getCounterClockWise()` (3)
/// after it, directly or through a local (`Direction d = context.getHorizontalDirection();`).
pub(crate) fn placement_turns(source: &str) -> Option<u8> {
    let i = source.find("getStateForPlacement(BlockPlaceContext")?;
    let open = i + source[i..].find('{')?;
    let body = balanced_braces(&source[open..])?;
    let turns = |e: &str| {
        ((e.matches(".getClockWise()").count()
            + 2 * e.matches(".getOpposite()").count()
            + 3 * e.matches(".getCounterClockWise()").count())
            % 4) as u8
    };
    for args in calls(body, "setValue") {
        let parts = split_top(&args);
        let [prop, value] = parts.as_slice() else {
            continue;
        };
        if !prop.trim().ends_with("FACING") {
            continue;
        }
        let value = value.trim();
        if value.contains("getHorizontalDirection()") {
            return Some(turns(value));
        }
        // A local set from the look: `direction.getOpposite()`.
        let var = value.split('.').next()?.trim();
        if var.is_empty() || !var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        let at = body.find(&format!(" {var} = "))?;
        let init = &body[at..at + body[at..].find(';')?];
        return init
            .contains("getHorizontalDirection()")
            .then(|| (turns(init) + turns(value)) % 4);
    }
    None
}

/// The argument text of each call to `name` in `source` (`name(` ... `)`).
pub(crate) fn calls(source: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = source[from..].find(name).map(|i| from + i) {
        let before = source[..i].chars().next_back();
        let after = &source[i + name.len()..];
        from = i + name.len();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let rest = after.trim_start();
        if !rest.starts_with('(') {
            continue;
        }
        if let Some(args) = balanced(rest) {
            out.push(args.to_string());
        }
    }
    out
}

/// What is inside the parentheses `text` opens with.
pub(crate) fn balanced(text: &str) -> Option<&str> {
    let mut depth = 0;
    let mut in_str = false;
    let mut prev = ' ';
    for (i, c) in text.char_indices() {
        if in_str {
            if c == '"' && prev != '\\' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&text[1..i]);
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    None
}

/// What is inside the braces `text` opens with.
pub(crate) fn balanced_braces(text: &str) -> Option<&str> {
    let mut depth = 0;
    for (i, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[1..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Arguments split at commas outside parentheses.
pub(crate) fn split_top(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in text.chars() {
        match c {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// An integer literal.
fn int(text: &str) -> Option<i32> {
    text.trim().parse().ok()
}

/// The first integer literal in an expression: `84 + row * 18` → 84.
fn first_int(text: &str) -> Option<i32> {
    let digits: String = text
        .trim()
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_blocks_state_definition() {
        let garage = "public GarageDoor(Properties properties) {
            super(properties);
            this.registerDefaultState(
                (BlockState)((BlockState)((BlockState)((BlockState)((BlockState)this.stateDefinition.any()).setValue(FACING, Direction.NORTH)).setValue(OPEN, true))
                    .setValue(POWERED, Boolean.FALSE))
                .setValue(PART, GarageDoor.GaragePart.TOP)
            );
        }
        protected void createBlockStateDefinition(Builder<Block, BlockState> builder) {
            builder.add(new Property[]{FACING, OPEN, POWERED, PART});
        }";
        let (fields, defaults) = state_definition(garage).unwrap();
        assert_eq!(fields, ["FACING", "OPEN", "POWERED", "PART"]);
        assert_eq!(
            defaults,
            [
                ("FACING".to_string(), "north".to_string()),
                ("OPEN".into(), "true".into()),
                ("POWERED".into(), "false".into()),
                ("PART".into(), "top".into()),
            ]
        );
        assert!(state_definition("class Plain {}").is_none());
    }

    #[test]
    fn reads_which_way_blocks_face_when_placed() {
        let curtain = "public BlockState getStateForPlacement(BlockPlaceContext context) {
            return this.defaultBlockState().setValue(FACING, context.getHorizontalDirection().getClockWise());
        }";
        assert_eq!(placement_turns(curtain), Some(1));
        let window = "public BlockState getStateForPlacement(BlockPlaceContext context) {
            return this.state(context).setValue(FACING, context.getHorizontalDirection());
        }";
        assert_eq!(placement_turns(window), Some(0));
        let local = "public BlockState getStateForPlacement(BlockPlaceContext ctx) {
            Direction direction = ctx.getHorizontalDirection();
            return this.defaultBlockState().setValue(HINGE, left).setValue(HORIZONTAL_FACING, direction.getOpposite());
        }";
        assert_eq!(placement_turns(local), Some(2));
        assert_eq!(placement_turns("no placement here"), None);
    }

    #[test]
    fn reads_mob_attributes_and_effects() {
        let m = mob(
            "Naga",
            "static AttributeSupplier.Builder createAttributes() { return Monster.createMonsterAttributes().add(Attributes.MAX_HEALTH, 10.0D).add(Attributes.MOVEMENT_SPEED, 0.23F); }",
        )
        .unwrap();
        assert_eq!(
            m.attributes,
            [
                ("minecraft:max_health".to_string(), 10.0),
                ("minecraft:movement_speed".to_string(), 0.23)
            ]
        );
        assert_eq!(
            effect("public ComfortEffect() { super(MobEffectCategory.BENEFICIAL, 0xB8B5F5); }"),
            Some(("beneficial".into(), 0xB8B5F5))
        );
        assert_eq!(color("-16777216"), Some(0));
    }

    #[test]
    fn reads_block_shapes() {
        let cable = block_shape(
            "Cable",
            Some("net.minecraft.world.level.block.PipeBlock"),
            "public Cable(Properties p) { super(4.0F, p); }",
        )
        .unwrap();
        assert_eq!(cable.pipe_thickness, Some(4.0));
        let panel = block_shape(
            "Panel",
            Some("Block"),
            "protected VoxelShape getShape(BlockState s, BlockGetter l, BlockPos p, CollisionContext c) { return SHAPE; }",
        )
        .unwrap();
        assert!(panel.custom_shape && panel.pipe_thickness.is_none());
        assert_eq!(block_shape("Cube", Some("Block"), "class Cube {}"), None);
    }

    #[test]
    fn reads_tanks_and_energy() {
        let source = r#"
            public static final int CAPACITY = 8_000;
            private final FluidStacksResourceHandler fluids = new FluidStacksResourceHandler(2, FluidType.BUCKET_VOLUME * 4);
            private final FluidTank lava = new FluidTank(CAPACITY);
            private final SimpleEnergyHandler energy = new SimpleEnergyHandler(10000, 200, 0);
        "#;
        let s = storage("Boiler", source).unwrap();
        assert_eq!(s.tanks, [8000, 4000, 4000]);
        assert_eq!(s.energy, Some([10000, 200, 0]));
        assert_eq!(storage("Plain", "class Plain {}"), None);
    }

    const MENU: &str = r#"
        addSlot(new SlotItemHandler(items, 0, 56, 35));
        addSlot(new OutputSlot(items, 1, 116, 35));
        for (int row = 0; row < 3; row++)
            for (int col = 0; col < 9; col++)
                addSlot(new Slot(inventory, col + row * 9 + 9, 8 + col * 18, 84 + row * 18));
        for (int col = 0; col < 9; col++) addSlot(new Slot(inventory, col, 8 + col * 18, 142));
        addDataSlots(data);
        this(id, inventory, new ItemStackHandler(2), new SimpleContainerData(2));
        static class OutputSlot extends SlotItemHandler {
            OutputSlot(ItemStackHandler items, int index, int x, int y) { super(items, index, x, y); }
            @Override public boolean mayPlace(ItemStack stack) { return false; }
        }
    "#;

    #[test]
    fn reads_a_menus_slots() {
        let m = menu("a.CrusherMenu", MENU);
        assert_eq!(
            m.slots,
            [
                MenuSlot {
                    index: Some(0),
                    x: 56,
                    y: 35,
                    result: false
                },
                MenuSlot {
                    index: Some(1),
                    x: 116,
                    y: 35,
                    result: true
                },
            ]
        );
        assert_eq!(m.inventory, Some([8, 84]));
        assert_eq!(m.data_slots, 2);
        assert!(!m.approximate);
    }

    #[test]
    fn reads_a_screen() {
        let s = screen(
            "a.CrusherScreen",
            r#"class CrusherScreen extends AbstractContainerScreen<CrusherMenu> {
                static final ResourceLocation T = ResourceLocation.fromNamespaceAndPath(Grind.ID, "textures/gui/crusher.png");
                CrusherScreen() { this.imageWidth = 176; this.imageHeight = 166; if (imageWidth == 3) {} }"#,
        );
        assert_eq!(s.menu.as_deref(), Some("CrusherMenu"));
        assert_eq!((s.width, s.height), (Some(176), Some(166)));
        assert_eq!(s.texture.as_deref(), Some("textures/gui/crusher.png"));
    }
}
