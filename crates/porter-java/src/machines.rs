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
    // `addStandardInventorySlots(inventory, 8, 84)`: vanilla's own helper says where.
    let standard: Option<[i32; 2]> =
        calls(source, "addStandardInventorySlots")
            .iter()
            .find_map(|args| {
                let parts = split_top(args);
                let n = parts.len();
                (n >= 3).then(|| Some([first_int(&parts[n - 2])?, first_int(&parts[n - 1])?]))?
            });
    // The player's inventory, by the constructor's `Inventory` parameter's name.
    let player_names = inventory_params(source);
    let is_player = |container: &str| {
        let c = container.trim().trim_start_matches("this.");
        if player_names.is_empty() {
            is_player_inventory(c)
        } else {
            player_names
                .iter()
                .any(|p| c == p || c.starts_with(&format!("{p}.")))
                || c.contains("player.getInventory()")
        }
    };
    // Slots placed by loops and locals (`inputStartX + column * borderSlotSize`),
    // evaluated as the constructor runs.
    let evaluated = evaluate_slots(source);
    if !evaluated.is_empty() {
        for (ty, container, index, x, y) in evaluated {
            if is_player(&container) {
                // Armour (36 to 39) and the off hand (40) sit apart from the inventory.
                if index.is_some_and(|i| i >= 36) {
                    continue;
                }
                inventory = Some(match inventory {
                    Some([ix, iy]) if iy <= y => [ix.min(x), iy],
                    _ => [x, y],
                });
                continue;
            }
            info.slots.push(MenuSlot {
                index,
                x,
                y,
                result: is_result(&ty, source),
            });
        }
        info.inventory = standard.or(inventory);
        if !calls(source, "addDataSlots").is_empty() {
            info.data_slots = calls(source, "SimpleContainerData")
                .iter()
                .filter_map(|a| int(a))
                .next()
                .unwrap_or(0) as usize;
        }
        return info;
    }
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
            if int(index).is_some_and(|i| i >= 36) {
                continue;
            }
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
    info.inventory = standard.or(inventory);
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
    let menu = [
        "AbstractContainerScreen<",
        "HandledScreen<",
        "AbstractRecipeBookScreen<",
        "AbstractFurnaceScreen<",
    ]
    .iter()
    .find_map(|p| {
        let at = source.find(p)? + p.len();
        let name: String = source[at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        (!name.is_empty()).then(|| name.rsplit('.').next().unwrap_or(&name).to_string())
    })
    // A library's own base (`AbstractWidgetsContainerScreen<InfuserMenu>`): the menu is
    // the first type its `extends` names.
    .or_else(|| {
        let simple = class.rsplit(['.', '$']).next().unwrap_or(class);
        let at = source.find(&format!("class {simple} extends "))?;
        let rest = &source[at..];
        let open = rest.find('<')?;
        // Only within the declaration's own line.
        if rest[..open].contains('{') {
            return None;
        }
        let name: String = rest[open + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        (!name.is_empty() && name != "T")
            .then(|| name.rsplit('.').next().unwrap_or(&name).to_string())
    });
    // `super(menu, inventory, title, 220, 185)`: a base that takes the size.
    let from_super: Option<[i32; 2]> = calls(source, "super").iter().find_map(|args| {
        let parts = split_top(args);
        let n = parts.len();
        if n < 5 {
            return None;
        }
        let (w, h) = (int(&parts[n - 2])?, int(&parts[n - 1])?);
        ((100..=512).contains(&w) && (60..=512).contains(&h)).then_some([w, h])
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
        width: assigned(&["imageWidth", "backgroundWidth"]).or(from_super.map(|s| s[0])),
        height: assigned(&["imageHeight", "backgroundHeight"]).or(from_super.map(|s| s[1])),
        texture,
    }
}

/// Whether a slot's container argument is the player's inventory.
/// The names of a menu's `Inventory` parameters (`Inventory playerInventory`).
fn inventory_params(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = source[from..].find("Inventory ").map(|i| from + i) {
        from = i + 10;
        let before = source[..i].chars().next_back();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
            continue;
        }
        let name: String = source[from..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let after = source[from + name.len()..].trim_start();
        if !name.is_empty()
            && (after.starts_with(',') || after.starts_with(')'))
            && !out.contains(&name)
        {
            out.push(name);
        }
    }
    out
}

/// Each `addSlot(new T(container, .., index, x, y))` of `source` as its code runs:
/// `int` locals and constants kept, `for (int v = a; v < b; v++)` loops unrolled,
/// the index and position evaluated. (slot type, container, index, x, y).
fn evaluate_slots(source: &str) -> Vec<(String, String, Option<i32>, i32, i32)> {
    let mut env: std::collections::HashMap<String, i64> = Default::default();
    // `static final int SLOT_SIZE = 18;` and the like, anywhere in the file.
    for stmt in source.split(';') {
        if let Some((lhs, rhs)) = stmt.split_once('=')
            && lhs.contains(" int ")
            && let Some(name) = lhs.split_whitespace().last()
            && let Some(v) = eval(rhs, &env)
        {
            env.insert(name.to_string(), v);
        }
    }
    let mut out = Vec::new();
    run(source, &mut env, &mut out, 0);
    out
}

fn run(
    text: &str,
    env: &mut std::collections::HashMap<String, i64>,
    out: &mut Vec<(String, String, Option<i32>, i32, i32)>,
    depth: usize,
) {
    if depth > 6 {
        return;
    }
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let next_for = rest.find("for (");
        let next_slot = rest.find("addSlot(");
        let next_int = rest.find("int ");
        let first = [next_for, next_slot, next_int].into_iter().flatten().min();
        let Some(at) = first else {
            break;
        };
        let here = &rest[at..];
        let word_before = rest[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        if Some(at) == next_for && !word_before {
            // `for (int v = a; v < b; v++) { body }` (or a one-statement body).
            let Some(head) = balanced(&here[4..]) else {
                i += at + 4;
                continue;
            };
            let after = &here[4 + head.len() + 2..];
            let parts: Vec<&str> = head.split(';').collect();
            let skip = after.len() - after.trim_start().len();
            let len = statement_len(&after[skip..]);
            let body_len = skip + len;
            let body: &str = {
                let t = &after[skip..skip + len];
                if t.starts_with('{') {
                    balanced_braces(t).unwrap_or("")
                } else {
                    t
                }
            };
            if let [init, cond, _] = parts.as_slice()
                && let Some((var, start)) = init
                    .trim()
                    .strip_prefix("int ")
                    .and_then(|r| r.split_once('='))
                && let Some(start) = eval(start, env)
            {
                let var = var.trim().to_string();
                let cond = cond.trim();
                let (limit, inclusive) = if let Some((_, l)) = cond.split_once("<=") {
                    (eval(l, env), true)
                } else if let Some((_, l)) = cond.split_once('<') {
                    (eval(l, env), false)
                } else {
                    (None, false)
                };
                if let Some(limit) = limit {
                    let end = if inclusive { limit + 1 } else { limit };
                    for v in start..end.min(start + 64) {
                        env.insert(var.clone(), v);
                        run(body, env, out, depth + 1);
                    }
                    env.remove(&var);
                }
            }
            i += at + 4 + head.len() + 2 + body_len;
            continue;
        }
        if Some(at) == next_slot && !word_before {
            if let Some(args) = balanced(&here[7..])
                && let Some(new) = args.trim().strip_prefix("new ")
                && let Some(open) = new.find('(')
                && let Some(inner) = balanced(&new[open..])
            {
                let ty = new[..open].trim().to_string();
                let parts = split_top(inner);
                let n = parts.len();
                if n >= 4
                    && let (Some(x), Some(y)) = (eval(&parts[n - 2], env), eval(&parts[n - 1], env))
                {
                    let index = eval(&parts[n - 3], env).map(|v| v as i32);
                    // The container is the argument before the index (a result slot takes the
                    // player first).
                    out.push((ty, parts[n - 4].clone(), index, x as i32, y as i32));
                }
                i += at + 7 + args.len() + 2;
                continue;
            }
            i += at + 8;
            continue;
        }
        if Some(at) == next_int && !word_before {
            // `int startX = 8;`
            let stmt = &here[4..here.find(';').unwrap_or(here.len())];
            if let Some((name, value)) = stmt.split_once('=')
                && let Some(v) = eval(value, env)
            {
                env.insert(name.trim().to_string(), v);
            }
            i += at + 4;
            continue;
        }
        i += at + 1;
    }
}

/// How long the statement `text` starts with is: a block, a `for` with its body,
/// or up to its `;`.
fn statement_len(text: &str) -> usize {
    if text.starts_with('{') {
        return balanced_braces(text).map_or(text.len(), |b| b.len() + 2);
    }
    if let Some(rest) = text.strip_prefix("for ")
        && let Some(head) = balanced(rest.trim_start())
    {
        let head_end = 4 + (rest.len() - rest.trim_start().len()) + head.len() + 2;
        let after = &text[head_end..];
        let skip = after.len() - after.trim_start().len();
        return head_end + skip + statement_len(&after[skip..]);
    }
    text.find(';').map_or(text.len(), |e| e + 1)
}

/// An int expression: literals, names in `env`, `+ - * /`, parentheses.
fn eval(text: &str, env: &std::collections::HashMap<String, i64>) -> Option<i64> {
    let t = text.trim();
    // `config.allowRepairing ? 23 : 34`: which holds isn't known here; the first.
    if let Some((_, choice)) = t.split_once('?')
        && let Some((first, second)) = choice.rsplit_once(':')
    {
        return eval(first, env).or_else(|| eval(second, env));
    }
    let tokens: Vec<String> = {
        let mut v = Vec::new();
        let mut cur = String::new();
        for c in t.chars() {
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
                cur.push(c);
            } else {
                if !cur.is_empty() {
                    v.push(std::mem::take(&mut cur));
                }
                if !c.is_whitespace() {
                    v.push(c.to_string());
                }
            }
        }
        if !cur.is_empty() {
            v.push(cur);
        }
        v
    };
    let mut pos = 0;
    let v = expr(&tokens, &mut pos, env)?;
    (pos == tokens.len()).then_some(v)
}

fn expr(
    t: &[String],
    pos: &mut usize,
    env: &std::collections::HashMap<String, i64>,
) -> Option<i64> {
    let mut v = term(t, pos, env)?;
    while let Some(op) = t.get(*pos).map(String::as_str) {
        match op {
            "+" => {
                *pos += 1;
                v += term(t, pos, env)?;
            }
            "-" => {
                *pos += 1;
                v -= term(t, pos, env)?;
            }
            _ => break,
        }
    }
    Some(v)
}

fn term(
    t: &[String],
    pos: &mut usize,
    env: &std::collections::HashMap<String, i64>,
) -> Option<i64> {
    let mut v = atom(t, pos, env)?;
    while let Some(op) = t.get(*pos).map(String::as_str) {
        match op {
            "*" => {
                *pos += 1;
                v *= atom(t, pos, env)?;
            }
            "/" => {
                *pos += 1;
                let d = atom(t, pos, env)?;
                v = v.checked_div(d)?;
            }
            "%" => {
                *pos += 1;
                let d = atom(t, pos, env)?;
                v = v.checked_rem(d)?;
            }
            _ => break,
        }
    }
    Some(v)
}

fn atom(
    t: &[String],
    pos: &mut usize,
    env: &std::collections::HashMap<String, i64>,
) -> Option<i64> {
    let tok = t.get(*pos)?.as_str();
    *pos += 1;
    match tok {
        "(" => {
            // A cast `(int)`, else a parenthesized expression.
            if t.get(*pos).map(String::as_str) == Some("int")
                && t.get(*pos + 1).map(String::as_str) == Some(")")
            {
                *pos += 2;
                return atom(t, pos, env);
            }
            let v = expr(t, pos, env)?;
            (t.get(*pos).map(String::as_str) == Some(")")).then(|| *pos += 1)?;
            Some(v)
        }
        "-" => Some(-atom(t, pos, env)?),
        _ => {
            if let Ok(v) = tok.trim_end_matches(['L', 'l']).parse::<i64>() {
                return Some(v);
            }
            let name = tok.strip_prefix("this.").unwrap_or(tok);
            let name = name.rsplit('.').next().unwrap_or(name);
            env.get(name).copied()
        }
    }
}

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
    // Item slots: the largest handler or list it makes.
    let mut items = 0usize;
    for name in [
        "new ItemStackHandler",
        "new SimpleContainer",
        "NonNullList.withSize",
        "new ItemStacksResourceHandler",
    ] {
        for args in calls(source, name) {
            // `new ItemStackHandler()` holds one.
            let n = if args.trim().is_empty() {
                Some(1)
            } else {
                split_top(&args).first().and_then(|a| constant(a, source))
            };
            if let Some(n) = n {
                items = items.max(n.clamp(0, 256) as usize);
            }
        }
    }
    (!tanks.is_empty() || energy.is_some() || items > 0).then(|| StorageInfo {
        class: class.to_string(),
        tanks,
        energy,
        items,
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
    // A renderer built on a base of the mod's own or a library's:
    // `class HandCrankRenderer extends KineticBlockEntityRenderer<HandCrankBlockEntity>`.
    // One that takes any entity (`class ShaftRenderer<T extends ..>`) names none.
    let simple = class.rsplit(['.', '$']).next()?;
    let at = source.find(&format!("class {simple} extends "))?;
    let decl = &source[at..];
    let decl = &decl[..decl.find('{')?];
    let open = decl.find("Renderer<")? + "Renderer<".len();
    let entity: String = decl[open..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        .collect();
    let entity = entity.rsplit('.').next().unwrap_or(&entity).to_string();
    (entity.len() > 1 && entity.starts_with(|c: char| c.is_ascii_uppercase())).then(|| {
        RendererInfo {
            class: class.to_string(),
            entity,
        }
    })
}

/// The property fields a block class adds, and its (field, default value) pairs.
pub(crate) type StateFields = (Vec<String>, Vec<(String, String)>);

/// A block class's state definition: the property fields its
/// `createBlockStateDefinition` adds (`builder.add(FACING, OPEN)`), and the values its
/// constructor's `registerDefaultState` sets, as (field, value) with the value as
/// a blockstate names it (`Direction.NORTH` is `north`, `GaragePart.TOP` `top`).
pub(crate) fn state_definition(source: &str) -> Option<StateFields> {
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
            let value = state_value(value).or_else(|| returned_constant(source, value));
            // `this.getServingsProperty()`: the field a method of its own returns.
            let field = constant(field)
                .or_else(|| returned_expression(source, field).and_then(|f| constant(&f)));
            let (Some(field), Some(value)) = (field, value) else {
                continue;
            };
            if !defaults.iter().any(|(f, _): &(String, String)| *f == field) {
                defaults.push((field, value));
            }
        }
    }
    (!fields.is_empty() || !defaults.is_empty()).then_some((fields, defaults))
}

/// The vanilla menu a block entity's `createMenu` opens over its own slots, with
/// that menu's slot count: `ChestMenu.threeRows(..)` is `minecraft:generic_9x3`
/// (27), `new HopperMenu(..)` `minecraft:hopper` (5), `new DispenserMenu(..)`
/// `minecraft:generic_3x3` (9), `new ChestMenu(MenuType.GENERIC_9x2, ..)` its type.
pub(crate) fn vanilla_menu(source: &str) -> Option<(String, usize)> {
    let at = source.find("createMenu(")?;
    // Its body: up to the next method, which is enough for these one-liners.
    let body = &source[at..];
    let body = &body[..body
        .find(
            "
    }",
        )
        .unwrap_or(body.len())
        .min(2000)];
    const ROWS: [(&str, usize); 6] = [
        ("oneRow", 1),
        ("twoRows", 2),
        ("threeRows", 3),
        ("fourRows", 4),
        ("fiveRows", 5),
        ("sixRows", 6),
    ];
    for (call, rows) in ROWS {
        if body.contains(&format!("ChestMenu.{call}(")) {
            return Some((format!("minecraft:generic_9x{rows}"), rows * 9));
        }
    }
    for rows in 1..=6 {
        if body.contains(&format!("MenuType.GENERIC_9x{rows}")) {
            return Some((format!("minecraft:generic_9x{rows}"), rows * 9));
        }
    }
    if body.contains("new HopperMenu(") {
        return Some(("minecraft:hopper".into(), 5));
    }
    if body.contains("new DispenserMenu(") || body.contains("MenuType.GENERIC_3x3") {
        return Some(("minecraft:generic_3x3".into(), 9));
    }
    None
}

/// Whether a block class's `getInteractionShape` returns the whole block
/// (`Shapes.block()`, or a constant of the class that is).
pub(crate) fn full_interaction(source: &str) -> bool {
    let Some(at) = source.find("getInteractionShape(") else {
        return false;
    };
    let body = &source[at..];
    let Some(ret) = body.find("return ").map(|i| &body[i + 7..]) else {
        return false;
    };
    let value = ret.split(';').next().unwrap_or("").trim();
    let value = value.trim_start_matches("(VoxelShape)").trim();
    if value == "Shapes.block()" {
        return true;
    }
    // `OUT_SHAPE`, with `OUT_SHAPE = Shapes.block();` in the class.
    let name = value.rsplit('.').next().unwrap_or(value);
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && source.contains(&format!("{name} = Shapes.block()"))
}

/// The constant a method of the class returns, for a value written as a call to
/// it: `Integer.valueOf(this.getMaxServings())` and `public int getMaxServings() {
/// return 4; }` give `4`.
fn returned_constant(source: &str, text: &str) -> Option<String> {
    let value = returned_expression(source, text)?;
    state_value(&value)
        .filter(|v| v.chars().all(|c| c.is_ascii_digit()) || v == "true" || v == "false")
}

/// What a method of the class returns, for an expression that calls it with no
/// arguments (`this.getMaxServings()`, `Integer.valueOf(getMaxServings())`): the
/// expression of its one `return`.
fn returned_expression(source: &str, text: &str) -> Option<String> {
    let t = text.trim();
    let t = t
        .strip_prefix("Integer.valueOf(")
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or(t)
        .trim();
    let t = t.strip_prefix("this.").unwrap_or(t);
    let name = t.strip_suffix("()")?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let at = source.find(&format!(" {name}() {{"))?;
    let body = &source[at..];
    let body = &body[body.find('{')? + 1..body.find('}')?];
    Some(
        body.trim()
            .strip_prefix("return ")?
            .strip_suffix(';')?
            .trim()
            .to_string(),
    )
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

/// How a block class says it is drawn, where it overrides `getRenderShape` with one
/// answer: `ENTITYBLOCK_ANIMATED` (its block entity draws it: a turning shaft),
/// `INVISIBLE`, `MODEL`. None where it doesn't, or answers by state.
pub(crate) fn render_shape(source: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = source[from..].find("getRenderShape(").map(|i| from + i) {
        from = at + 1;
        let rest = &source[at..];
        // A declaration: a body opens before any statement ends.
        let Some(open) = rest.find('{') else {
            break;
        };
        if rest[..open].contains(';') {
            continue;
        }
        let mut depth = 0usize;
        let mut end = rest.len();
        for (i, c) in rest[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &rest[open..end];
        let mut shapes: Vec<&str> = body
            .split("RenderShape.")
            .skip(1)
            .map(|s| {
                let n = s
                    .find(|c: char| !(c.is_ascii_uppercase() || c == '_'))
                    .unwrap_or(s.len());
                &s[..n]
            })
            .filter(|s| !s.is_empty())
            .collect();
        shapes.dedup();
        if let [one] = shapes[..] {
            return Some(one.to_string());
        }
        return None;
    }
    // A model class of the mod's own that draws nothing of the block in the world
    // (its `getQuads` answers an empty list there, and gathers no faces of its own):
    // the block it is swapped in for is drawn by its block entity (Create's shaft, by
    // its renderer or a Flywheel visual).
    if source.contains("BakedModel")
        && source.contains("getQuads(")
        && source.contains("Collections.emptyList()")
        && !["addAll(", "new ArrayList", ".add("]
            .iter()
            .any(|m| source.contains(m))
    {
        return Some("ENTITYBLOCK_ANIMATED".to_string());
    }
    None
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
    fn reads_how_a_block_says_it_is_drawn() {
        let animated = "class Crank extends Block {\n    @Override\n    public RenderShape getRenderShape(BlockState state) {\n        return RenderShape.ENTITYBLOCK_ANIMATED;\n    }\n}";
        assert_eq!(
            render_shape(animated).as_deref(),
            Some("ENTITYBLOCK_ANIMATED")
        );
        // By state: no one answer.
        let by_state = "public RenderShape getRenderShape(BlockState s) { return s.getValue(CASING) ? RenderShape.MODEL : RenderShape.ENTITYBLOCK_ANIMATED; }";
        assert_eq!(render_shape(by_state), None);
        // A call, not an override.
        assert_eq!(
            render_shape("if (state.getRenderShape() == RenderShape.MODEL) { draw(); }"),
            None
        );
        // A model class that draws nothing of the block in the world.
        let empty = "class ShaftModel extends BakedModelWrapper<BakedModel> { public List<BakedQuad> getQuads(BlockState s) { if (!virtual) return Collections.emptyList(); return super.getQuads(s); } }";
        assert_eq!(render_shape(empty).as_deref(), Some("ENTITYBLOCK_ANIMATED"));
        let adds = "class TankModel extends BakedModelWrapper<BakedModel> { public List<BakedQuad> getQuads(BlockState s) { if (side != null) return Collections.emptyList(); quads.addAll(super.getQuads(s)); return quads; } }";
        assert_eq!(render_shape(adds), None);
    }

    #[test]
    fn reads_a_librarys_screen_and_a_menu_with_armour_slots() {
        let menu_src = r#"
public class InfuserMenu extends AbstractContainerMenu {
   public InfuserMenu(int containerId, final Inventory inventory, Container container) {
      super(TYPE, containerId);
      this.addSlot(new Slot(container, 0, 8, this.getConfig().allowRepairing.isActive() ? 23 : 34) {
      });
      for (int k = 0; k < 4; k++) {
         this.addSlot(new ArmorSlot(inventory, inventory.player, slot, 39 - k, 8 + 188 * (k / 2), 103 + k % 2 * 18, identifier));
      }
      this.addStandardInventorySlots(inventory, 30, 103);
      this.addSlot(new Slot(inventory, 40, 8, 161) {
      });
   }
}
"#;
        let m = menu("a.InfuserMenu", menu_src);
        assert_eq!(m.inventory, Some([30, 103]), "{m:?}");
        assert_eq!(m.slots.len(), 1, "{m:?}");
        assert_eq!((m.slots[0].x, m.slots[0].y), (8, 23));
        let screen_src = r#"
public class InfuserScreen extends AbstractWidgetsContainerScreen<InfuserMenu> implements ContainerListener {
   public static final Identifier TEXTURE_LOCATION = EnchantingInfuser.id("textures/gui/container/infuser.png");
   public InfuserScreen(InfuserMenu infuserMenu, Inventory inventory, Component title) {
      super(infuserMenu, inventory, title, 220, 185);
      this.inventoryLabelX = 30;
   }
}
"#;
        let s = screen("a.InfuserScreen", screen_src);
        assert_eq!(s.menu.as_deref(), Some("InfuserMenu"));
        assert_eq!((s.width, s.height), (Some(220), Some(185)));
        assert_eq!(
            s.texture.as_deref(),
            Some("textures/gui/container/infuser.png")
        );
    }

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
        // A feast starts whole: its default is what a method of its own returns.
        let feast = "public FeastBlock(Properties properties) {
            super(properties);
            this.registerDefaultState((BlockState)((BlockState)this.stateDefinition.any()).setValue(this.getServingsProperty(), Integer.valueOf(this.getMaxServings())));
        }
        public IntegerProperty getServingsProperty() {
            return SERVINGS;
        }
        public int getMaxServings() {
            return 4;
        }";
        let (_, defaults) = state_definition(feast).unwrap();
        assert_eq!(defaults, [("SERVINGS".to_string(), "4".to_string())]);
    }

    #[test]
    fn reads_a_vanilla_menu() {
        let cabinet = "protected AbstractContainerMenu createMenu(int id, Inventory player) {
        return ChestMenu.threeRows(id, player, this);
    }";
        assert_eq!(
            vanilla_menu(cabinet),
            Some(("minecraft:generic_9x3".into(), 27))
        );
        let hopper = "public AbstractContainerMenu createMenu(int id, Inventory inv) {
        return new HopperMenu(id, inv, this);
    }";
        assert_eq!(vanilla_menu(hopper), Some(("minecraft:hopper".into(), 5)));
        let own = "public AbstractContainerMenu createMenu(int id, Inventory inv, Player p) {
        return new CrusherMenu(id, inv, this);
    }";
        assert_eq!(vanilla_menu(own), None);
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
    fn reads_slots_placed_by_locals_and_loops() {
        // Farmer's Delight's cooking pot: its own handler is `this.inventory`, the
        // player's is the `Inventory` parameter.
        let pot = "public CookingPotMenu(int windowId, Inventory playerInventory, CookingPotBlockEntity blockEntity, ContainerData data) {
      this.inventory = blockEntity.getInventory();
      int startX = 8;
      int startY = 18;
      int inputStartX = 30;
      int inputStartY = 17;
      int borderSlotSize = 18;

      for (int row = 0; row < 2; row++) {
         for (int column = 0; column < 3; column++) {
            this.addSlot(new ItemHandlerSlot(this.inventory, row * 3 + column, inputStartX + column * borderSlotSize, inputStartY + row * borderSlotSize));
         }
      }

      this.addSlot(new CookingPotMealSlot(this.inventory, 6, 124, 26));
      this.addSlot(new CookingPotResultSlot(playerInventory.player, blockEntity, this.inventory, 8, 124, 55));
      int startPlayerInvY = startY * 4 + 12;

      for (int row = 0; row < 3; row++) {
         for (int column = 0; column < 9; column++) {
            this.addSlot(new Slot(playerInventory, 9 + row * 9 + column, startX + column * borderSlotSize, startPlayerInvY + row * borderSlotSize));
         }
      }
   }";
        let m = menu("a.CookingPotMenu", pot);
        let at: Vec<(Option<i32>, i32, i32)> =
            m.slots.iter().map(|s| (s.index, s.x, s.y)).collect();
        assert_eq!(
            at,
            [
                (Some(0), 30, 17),
                (Some(1), 48, 17),
                (Some(2), 66, 17),
                (Some(3), 30, 35),
                (Some(4), 48, 35),
                (Some(5), 66, 35),
                (Some(6), 124, 26),
                (Some(8), 124, 55),
            ]
        );
        assert!(m.slots[7].result);
        assert_eq!(m.inventory, Some([8, 84]));
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
