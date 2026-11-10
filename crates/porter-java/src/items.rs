//! What a mod's items are made of, read from its source: a registration that only
//! names a constant (`ITEMS.register("x", () -> ModItems.X)`) gets the constant's
//! initializer, an item class's constructor gets the `super(..)` it passes on
//! (`properties.pickaxe(material, damage, speed).fireResistant()`), and the mod's
//! tool and armor materials are read into numbers, so the content mod can give its
//! tools and armor the stats Java gives them.

use porter_model::{ArmorMaterialInfo, ModScan, ToolMaterialInfo};

use crate::machines::{balanced, calls};

/// Fills in `scan`'s item registrations and materials from `sources` (class name,
/// source text).
pub(crate) fn resolve(sources: &[(String, String)], scan: &mut ModScan) {
    let simple_source = |simple: &str| {
        sources
            .iter()
            .find(|(name, _)| name.rsplit(['.', '$']).next() == Some(simple))
            .map(|(_, t)| t.as_str())
    };
    for reg in scan
        .registrations
        .iter_mut()
        .filter(|r| r.registry == "item")
    {
        // `() -> ModItems.NETHERITE_DIAMOND_PICKAXE`: the constant's initializer.
        // The supplier, the registration's last argument: `"x", () -> ModItems.X`.
        let last = split_args(&reg.args).pop().unwrap_or_default();
        let t = last.trim();
        let t = t.strip_prefix("() ->").unwrap_or(t).trim();
        if let Some((class, field)) = t.split_once('.')
            && is_ident(class)
            && is_ident(field)
            && field
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            && let Some(init) = simple_source(class).and_then(|src| field_init(src, field))
        {
            reg.args = init;
            if reg.creates.is_none() {
                reg.creates = new_class(&reg.args);
            }
        }
        // `new AdvancedPickaxeItem(material, 1.0F, -2.8F, props)`: what its
        // constructor passes to `super`, with the arguments in place.
        if let Some(class) = new_class(&reg.args)
            && let Some(src) = simple_source(&class)
            && let Some(args) = call_args(&reg.args, &format!("new {class}"))
            && let Some(sup) = super_call(src, &class, &args)
        {
            reg.args = format!("{}\n/* super */ {sup}", reg.args);
        }
        // `bowlFoodItem(FoodValues.BEEF_STEW)`: a static helper of the mod that makes
        // the item's settings (`return new Properties().food(food).craftRemainder(
        // Items.BOWL).stacksTo(16);`), its settings added, as they read in Java.
        let mut inlined = Vec::new();
        let mut text = reg.args.clone();
        for _ in 0..3 {
            let mut added = String::new();
            for (name, args) in helper_calls(&text) {
                if inlined.contains(&(name.clone(), args.len())) {
                    continue;
                }
                let Some((params, ret)) = sources
                    .iter()
                    .find_map(|(_, src)| properties_helper(src, &name, args.len()))
                else {
                    continue;
                };
                inlined.push((name.clone(), args.len()));
                added.push_str(&format!(
                    "\n/* {name} */ {}",
                    substitute(&ret, &params, &args)
                ));
            }
            if added.is_empty() {
                break;
            }
            reg.args.push_str(&added);
            text = added;
        }
    }
    // Foods and consumables: the components their constants build.
    for (_, src) in sources {
        for (field, init) in typed_fields(src, "FoodProperties") {
            if let Some(json) = food(src, &init) {
                let owner = enclosing_class(src, &field);
                scan.foods
                    .push((format!("{owner}.{field}"), json.to_string()));
            }
        }
        for (field, init) in typed_fields(src, "Consumable") {
            if let Some(json) = consumable(src, &init) {
                let owner = enclosing_class(src, &field);
                scan.consumables
                    .push((format!("{owner}.{field}"), json.to_string()));
            }
        }
    }
    for (class, src) in sources {
        let owner = class.rsplit(['.', '$']).next().unwrap_or(class);
        for (field, init) in typed_fields(src, "ToolMaterial") {
            let init = inline_helper(src, &init);
            if let Some(m) = tool_material(sources, &format!("{owner}.{field}"), &init) {
                scan.tool_materials.push(m);
            }
        }
        for (field, init) in typed_fields(src, "ArmorMaterial") {
            let init = inline_helper(src, &init);
            if let Some(m) = armor_material(sources, &format!("{owner}.{field}"), &init) {
                scan.armor_materials.push(m);
            }
        }
    }
}

/// A tool material's mining speed as the mod's code multiplies it by a config value
/// for that material (Advanced Netherite's `getDestroySpeed`: `if
/// (material.isMaterial(ModToolMaterials.NETHERITE_IRON)) newSpeed *=
/// config.getIronBreakingSpeedMultiplier();`), at the config's default: Horizon has
/// no hook for a tool's speed, so it is baked in.
pub(crate) fn speed_multipliers(sources: &[(String, String)], scan: &mut ModScan) {
    let mut done = std::collections::BTreeSet::new();
    for (_, src) in sources {
        let mut from = 0;
        while let Some(i) = src[from..].find("isMaterial(").map(|i| from + i) {
            from = i + 11;
            let Some(arg) = balanced(&src[i + 10..]) else {
                continue;
            };
            let arg = arg.trim().to_string();
            // The block the test guards: its first statement that multiplies a speed.
            let rest = &src[from..];
            let window = &rest[..rest.len().min(400)];
            let Some(line) = window
                .lines()
                .find(|l| l.contains("*=") && l.to_ascii_lowercase().contains("speed"))
            else {
                continue;
            };
            // `config.getIronBreakingSpeedMultiplier()` → `ironBreakingSpeedMultiplier`.
            let Some(get) = line.rsplit(".get").next().and_then(|g| g.split('(').next()) else {
                continue;
            };
            let key = {
                let mut c = get.chars();
                match c.next() {
                    Some(f) => f.to_ascii_lowercase().to_string() + c.as_str(),
                    None => continue,
                }
            };
            let Some(factor) = scan
                .config
                .iter()
                .find(|e| e.path.rsplit('.').next() == Some(key.as_str()))
                .and_then(|e| e.default.trim().parse::<f32>().ok())
            else {
                continue;
            };
            if let Some(m) = scan.tool_materials.iter_mut().find(|m| {
                m.name == arg
                    || arg.ends_with(&format!(".{}", m.name))
                    || m.name.ends_with(&format!(".{arg}"))
            }) && done.insert(m.name.clone())
            {
                m.speed *= factor;
            }
        }
    }
}

/// The innermost class declared before the field `field`'s declaration in `src`
/// (`ConsumableValues` for a constant of a nested class).
fn enclosing_class(src: &str, field: &str) -> String {
    let at = src.find(&format!(" {field} =")).unwrap_or(src.len());
    let before = &src[..at];
    // The last `class X` whose body is still open at the field.
    let mut best = String::new();
    let mut from = 0;
    while let Some(i) = before[from..].find("class ").map(|i| from + i) {
        from = i + 6;
        let name: String = before[from..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let Some(open) = before[from..].find('{').map(|o| from + o) else {
            continue;
        };
        let body = &before[open..];
        let depth = body.matches('{').count() as i64 - body.matches('}').count() as i64;
        if depth > 0 && !name.is_empty() {
            best = name;
        }
    }
    best
}

/// A number, or a constant of `src` that holds one (`BRIEF_DURATION = 600`).
fn number_in(src: &str, text: &str) -> Option<f64> {
    let t = text.trim().trim_end_matches(['f', 'F', 'd', 'D', 'L']);
    if let Ok(v) = t.parse() {
        return Some(v);
    }
    let name = t.rsplit('.').next()?;
    let init = field_init(src, name)?;
    let i = init.trim().trim_end_matches(['f', 'F', 'd', 'D', 'L']);
    i.parse().ok()
}

/// `new FoodProperties.Builder().nutrition(12).saturationModifier(0.8F).alwaysEdible().build()`
/// as the `minecraft:food` component (saturation is nutrition × modifier × 2).
fn food(src: &str, init: &str) -> Option<serde_json::Value> {
    if !init.contains("nutrition(") && !init.contains("alwaysEdible") {
        return None;
    }
    let arg = |call: &str| calls(init, call).first().and_then(|a| number_in(src, a));
    let nutrition = arg("nutrition").unwrap_or(0.0);
    let modifier = arg("saturationModifier").unwrap_or(0.0);
    Some(serde_json::json!({
        "nutrition": nutrition as i64,
        "saturation": nutrition * modifier * 2.0,
        "can_always_eat": init.contains("alwaysEdible()"),
    }))
}

/// `Consumables.defaultDrink().onConsume(new ApplyStatusEffectsConsumeEffect(..)).build()`
/// as the `minecraft:consumable` component: vanilla's effect kinds only; one of the
/// mod's own (a heal) is left out.
fn consumable(src: &str, init: &str) -> Option<serde_json::Value> {
    let drink = init.contains("defaultDrink()");
    if !drink && !init.contains("defaultFood()") && !init.contains("Consumable.builder()") {
        return None;
    }
    let mut c = serde_json::json!({
        "consume_seconds": 1.6,
        "animation": if drink { "drink" } else { "eat" },
        "sound": if drink { "minecraft:entity.generic.drink" } else { "minecraft:entity.generic.eat" },
        "has_consume_particles": !drink,
        "on_consume_effects": [],
    });
    if let Some(s) = calls(init, "consumeSeconds")
        .first()
        .and_then(|a| number_in(src, a))
    {
        c["consume_seconds"] = s.into();
    }
    let effect_id = |text: &str| -> Option<String> {
        let t = text.trim();
        let (class, name) = t.rsplit_once('.')?;
        let id = name.to_ascii_lowercase();
        Some(if class.ends_with("MobEffects") {
            format!("minecraft:{id}")
        } else {
            id
        })
    };
    let mut effects = Vec::new();
    for on in calls(init, "onConsume") {
        let on = on.trim();
        if let Some(args) = call_args(on, "new ApplyStatusEffectsConsumeEffect") {
            let mut list = Vec::new();
            for inst in calls(&args.join(","), "new MobEffectInstance") {
                let parts = split_args(&inst);
                let Some(id) = parts.first().and_then(|p| effect_id(p)) else {
                    continue;
                };
                let duration = parts.get(1).and_then(|p| number_in(src, p)).unwrap_or(0.0);
                let amplifier = parts.get(2).and_then(|p| number_in(src, p)).unwrap_or(0.0);
                list.push(serde_json::json!({
                    "id": id,
                    "duration": duration as i64,
                    "amplifier": amplifier as i64,
                }));
            }
            // A probability after the instance (`new ApplyStatusEffectsConsumeEffect(instance, 1.0F)`).
            let probability = args
                .last()
                .filter(|a| !a.contains("MobEffectInstance"))
                .and_then(|a| number_in(src, a))
                .unwrap_or(1.0);
            if !list.is_empty() {
                effects.push(serde_json::json!({
                    "type": "minecraft:apply_effects",
                    "effects": list,
                    "probability": probability,
                }));
            }
        } else if let Some(args) = call_args(on, "new RemoveStatusEffectsConsumeEffect") {
            if let Some(id) = args.first().and_then(|a| effect_id(a)) {
                effects
                    .push(serde_json::json!({ "type": "minecraft:remove_effects", "effects": id }));
            }
        } else if on.starts_with("new ClearAllStatusEffectsConsumeEffect") {
            effects.push(serde_json::json!({ "type": "minecraft:clear_all_effects" }));
        }
    }
    c["on_consume_effects"] = serde_json::Value::Array(effects);
    Some(c)
}

fn is_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `new AdvancedPickaxeItem(...)` → `AdvancedPickaxeItem`.
fn new_class(text: &str) -> Option<String> {
    let at = text.find("new ")? + 4;
    let name: String = text[at..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        .collect();
    let simple = name.rsplit('.').next()?.to_string();
    (!simple.is_empty()).then_some(simple)
}

/// The arguments of the first call to `callee` in `text`, split at top-level commas.
fn call_args(text: &str, callee: &str) -> Option<Vec<String>> {
    let at = text.find(callee)? + callee.len();
    Some(split_args(balanced(text[at..].trim_start())?))
}

/// Arguments split at commas outside brackets and generics (`EnumMap<ArmorType,
/// Integer>`); a lambda's `->` and a comparison's `<` are left alone.
pub(crate) fn split_args(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut angles = 0i32;
    let mut cur = String::new();
    let mut in_str = false;
    let mut prev = ' ';
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            if c == '"' && prev != '\\' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                // A type's `<`: right after a name, before a type or `?`.
                '<' if prev.is_ascii_alphanumeric()
                    && chars
                        .get(i + 1)
                        .is_some_and(|n| n.is_ascii_uppercase() || *n == '?') =>
                {
                    angles += 1
                }
                '>' if angles > 0 && prev != '-' => angles -= 1,
                ',' if depth == 0 && angles == 0 => {
                    out.push(cur.trim().to_string());
                    cur.clear();
                    prev = c;
                    continue;
                }
                _ => {}
            }
        }
        cur.push(c);
        prev = c;
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// The initializer of static field `field` in `src`: `X = <init>;`.
fn field_init(src: &str, field: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = src[from..].find(field).map(|i| from + i) {
        from = i + field.len();
        let before = src[..i].chars().next_back();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let rest = src[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        if rest.starts_with('=') {
            continue;
        }
        return Some(until_semicolon(rest).trim().to_string());
    }
    None
}

/// `text` up to its first `;` outside brackets and strings.
fn until_semicolon(text: &str) -> &str {
    let mut depth = 0i32;
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
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ';' if depth == 0 => return &text[..i],
                _ => {}
            }
        }
        prev = c;
    }
    text
}

/// The static fields of type `ty` in `src`, with their initializers.
fn typed_fields(src: &str, ty: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let needle = format!(" {ty} ");
    let mut from = 0;
    while let Some(i) = src[from..].find(&needle).map(|i| from + i) {
        from = i + needle.len();
        let line_start = src[..i].rfind('\n').map_or(0, |n| n + 1);
        if !src[line_start..i].contains("static") {
            continue;
        }
        let rest = &src[from..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let after = rest[name.len()..].trim_start();
        if let Some(init) = after.strip_prefix('=') {
            out.push((name, until_semicolon(init).trim().to_string()));
        }
    }
    out
}

/// A method of `src` (`name(params) { ... }`): its parameter names and body.
fn method(src: &str, name: &str) -> Option<(Vec<String>, String)> {
    let mut from = 0;
    while let Some(i) = src[from..].find(&format!("{name}(")).map(|i| from + i) {
        from = i + name.len();
        let before = src[..i].chars().next_back();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
            continue;
        }
        let params = balanced(&src[i + name.len()..])?;
        let after = &src[i + name.len() + params.len() + 2..];
        let after_trim = after.trim_start();
        // A declaration: its body follows (maybe after `throws ..`); a call is
        // followed by `;`, `.`, `)` or `,`.
        let Some(brace) = after_trim.find('{') else {
            continue;
        };
        if after_trim[..brace].contains([';', '.', ')', ',', '=']) {
            continue;
        }
        let body_start = after.len() - after_trim.len() + brace;
        let body = crate::machines::balanced_braces(&after[body_start..])?;
        let names = split_args(params)
            .iter()
            .filter_map(|p| p.split_whitespace().last().map(str::to_string))
            .collect();
        return Some((names, body.to_string()));
    }
    None
}

/// The calls in `text` to a plain function (`bowlFoodItem(a, b)`, not `x.f(..)` or
/// `new X(..)`), with their arguments.
fn helper_calls(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if !(c.is_ascii_lowercase() || c == '_')
            || (i > 0 && {
                let b = bytes[i - 1] as char;
                b.is_ascii_alphanumeric() || b == '_' || b == '.' || b == ':'
            })
        {
            i += 1;
            continue;
        }
        let name: String = text[i..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let after = i + name.len();
        let preceded_by_new = text[..i].trim_end().ends_with("new");
        if text[after..].starts_with('(')
            && !preceded_by_new
            && name != "new"
            && name != "super"
            && let Some(args) = balanced(&text[after..])
        {
            let args: Vec<String> = split_args(args)
                .into_iter()
                .filter(|a| !a.trim().is_empty())
                .collect();
            out.push((name.clone(), args));
        }
        i = after.max(i + 1);
    }
    out
}

/// A static method `name` of `src` with `arity` parameters that returns an item's
/// settings (`new Properties()...`, or another such helper): its parameter names and
/// the expression it returns.
fn properties_helper(src: &str, name: &str, arity: usize) -> Option<(Vec<String>, String)> {
    let mut from = 0;
    while let Some(i) = src[from..].find(&format!(" {name}(")).map(|i| from + i) {
        from = i + 1;
        // A declaration returning `Properties` / `Item.Properties`.
        let line_start = src[..i].rfind(['\n', ';', '}', '{']).map_or(0, |n| n + 1);
        let head = &src[line_start..i];
        if !head.contains("static") || !head.trim_end().ends_with("Properties") {
            continue;
        }
        let Some((params, body)) = method(&src[i..], name) else {
            continue;
        };
        if params.len() != arity {
            continue;
        }
        let ret = body
            .find("return ")
            .map(|r| until_semicolon(&body[r + 7..]))?;
        return Some((params, ret.trim().to_string()));
    }
    None
}

/// `text` with each whole-word `params[i]` replaced by `args[i]`.
fn substitute(text: &str, params: &[String], args: &[String]) -> String {
    let mut out = text.to_string();
    for (p, a) in params.iter().zip(args) {
        let mut next = String::new();
        let mut rest = out.as_str();
        while let Some(i) = rest.find(p.as_str()) {
            let before = rest[..i].chars().next_back();
            let after = rest[i + p.len()..].chars().next();
            let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
            let dotted = before == Some('.');
            next.push_str(&rest[..i]);
            if word(before) || word(after) || dotted {
                next.push_str(p);
            } else {
                next.push_str(a);
            }
            rest = &rest[i + p.len()..];
        }
        next.push_str(rest);
        out = next;
    }
    out
}

/// What `class`'s constructor taking `args` passes to `super(..)`, with its
/// parameters replaced by `args`, and a static helper of the class it calls
/// inlined (Advanced Netherite's `armorProperties(material, armorType, properties)`).
fn super_call(src: &str, class: &str, args: &[String]) -> Option<String> {
    let mut from = 0;
    while let Some(i) = src[from..]
        .find(&format!("public {class}("))
        .map(|i| from + i)
    {
        from = i + 1;
        let (params, body) = method(&src[i + 7..], class)?;
        if params.len() != args.len() {
            continue;
        }
        let sup = calls(&body, "super").into_iter().next()?;
        let sup = substitute(&sup, &params, args);
        return Some(inline_helper(src, &sup));
    }
    None
}

/// `text` with a call to a static helper of `src` (`material(a, b)`, `register(..)`)
/// replaced by what the helper returns, its parameters replaced and its locals
/// (`Holder<SoundEvent> equipSound = SoundEvents.ARMOR_EQUIP_NETHERITE;`) too.
fn inline_helper(src: &str, text: &str) -> String {
    let t = text.trim();
    let name: String = t
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() || name == "new" {
        return text.to_string();
    }
    let Some(args) = balanced(&t[name.len()..]).map(split_args) else {
        return text.to_string();
    };
    let Some((params, body)) = method(src, &name) else {
        return text.to_string();
    };
    let Some(ret) = body
        .find("return ")
        .map(|i| until_semicolon(&body[i + 7..]))
    else {
        return text.to_string();
    };
    // Locals before the return: `Type name = value;`.
    let mut names = params.clone();
    let mut values = args.clone();
    for stmt in body[..body.find("return ").unwrap_or(0)].split(';') {
        let stmt = stmt.trim();
        if let Some((lhs, rhs)) = stmt.split_once('=')
            && let Some(local) = lhs.split_whitespace().last()
            && is_ident(local)
        {
            names.push(local.to_string());
            values.push(substitute(rhs.trim(), &params, &args));
        }
    }
    let ret = substitute(ret, &names, &values);
    // The tail of the expression (`armorProperties(..).fireResistant()`) kept.
    let tail = &t[name.len() + 2 + balanced(&t[name.len()..]).map_or(0, str::len)..];
    format!("{}{tail}", ret.trim())
}

/// A number literal: `2531`, `9.0F`, `-2.8F`.
fn number(text: &str) -> Option<f32> {
    text.trim()
        .trim_end_matches(['f', 'F', 'd', 'D', 'L'])
        .parse()
        .ok()
}

/// A tag or resource id an expression names: `BlockTags.INCORRECT_FOR_NETHERITE_TOOL`
/// → `minecraft:incorrect_for_netherite_tool`; a constant of the mod's (`ModTags.X`)
/// → the string its initializer names (`"incorrect_for_x"`, the mod's own namespace
/// when it has none).
pub(crate) fn resource_id(sources: &[(String, String)], expr: &str) -> Option<String> {
    let t = expr.trim();
    for vanilla in [
        "BlockTags.",
        "ItemTags.",
        "EquipmentAssets.",
        "Tags.Items.",
        "Tags.Blocks.",
    ] {
        if let Some(name) = t.strip_prefix(vanilla)
            && is_ident(name)
        {
            let ns = if vanilla.starts_with("Tags.") {
                "c"
            } else {
                "minecraft"
            };
            return Some(format!("{ns}:{}", name.to_ascii_lowercase()));
        }
    }
    let (class, field) = t.rsplit_once('.')?;
    let class = class.rsplit('.').next()?;
    let src = sources
        .iter()
        .find(|(name, _)| name.rsplit(['.', '$']).next() == Some(class))?
        .1
        .as_str();
    let init = field_init(src, field)?;
    let init = inline_helper(src, &init);
    // The id's string literals: one `"ns:path"`, or a namespace and a path.
    let strings: Vec<&str> = init.split('"').skip(1).step_by(2).collect();
    match strings.as_slice() {
        [one] => Some(one.to_string()),
        [.., ns, path] if !ns.contains(':') && !path.contains(':') => Some(format!("{ns}:{path}")),
        [.., last] => Some(last.to_string()),
        [] => None,
    }
}

fn tool_material(sources: &[(String, String)], name: &str, init: &str) -> Option<ToolMaterialInfo> {
    let args = call_args(init, "new ToolMaterial")?;
    let [incorrect, durability, speed, bonus, enchantment, repair] = args.as_slice() else {
        return None;
    };
    Some(ToolMaterialInfo {
        name: name.to_string(),
        incorrect: resource_id(sources, incorrect),
        durability: number(durability)? as u32,
        speed: number(speed)?,
        damage_bonus: number(bonus)?,
        enchantment: number(enchantment).unwrap_or(0.0) as u32,
        repair: resource_id(sources, repair),
    })
}

fn armor_material(
    sources: &[(String, String)],
    name: &str,
    init: &str,
) -> Option<ArmorMaterialInfo> {
    let args = call_args(init, "new ArmorMaterial")?;
    let [
        durability,
        defense,
        enchantment,
        sound,
        toughness,
        knockback,
        repair,
        asset,
    ] = args.as_slice()
    else {
        return None;
    };
    // `attribute.put(ArmorType.BOOTS, 4)` or `Map.of(ArmorType.BOOTS, 4, ...)`.
    let mut pieces = Vec::new();
    for put in calls(defense, "put") {
        if let [k, v] = split_args(&put).as_slice()
            && let (Some(k), Some(v)) = (k.strip_prefix("ArmorType."), number(v))
        {
            pieces.push((k.to_ascii_lowercase(), v as i32));
        }
    }
    if pieces.is_empty() {
        let parts = split_args(calls(defense, "of").first().map_or("", |s| s.as_str()));
        for pair in parts.chunks(2) {
            if let [k, v] = pair
                && let (Some(k), Some(v)) = (k.strip_prefix("ArmorType."), number(v))
            {
                pieces.push((k.to_ascii_lowercase(), v as i32));
            }
        }
    }
    let sound = sound
        .trim()
        .strip_prefix("SoundEvents.ARMOR_EQUIP_")
        .map(|s| format!("minecraft:item.armor.equip_{}", s.to_ascii_lowercase()));
    Some(ArmorMaterialInfo {
        name: name.to_string(),
        durability: number(durability)? as u32,
        defense: pieces,
        enchantment: number(enchantment).unwrap_or(0.0) as u32,
        sound,
        toughness: number(toughness).unwrap_or(0.0),
        knockback: number(knockback).unwrap_or(0.0),
        repair: resource_id(sources, repair),
        asset: resource_id(sources, asset),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use porter_model::Registration;

    fn sources() -> Vec<(String, String)> {
        vec![
            (
                "a.ModItems".into(),
                "public class ModItems {
   public static final AdvancedPickaxeItem NETHERITE_DIAMOND_PICKAXE = new AdvancedPickaxeItem(
      ModToolMaterials.NETHERITE_DIAMOND, 1.0F, -2.8F, new Properties().setId(key(\"netherite_diamond_pickaxe\"))
   );
   public static final AdvancedArmorItem NETHERITE_DIAMOND_HELMET = new AdvancedArmorItem(
      ModArmorMaterials.NETHERITE_DIAMOND, ArmorType.HELMET, new Properties().setId(key(\"netherite_diamond_helmet\"))
   );
}"
                .into(),
            ),
            (
                "a.AdvancedPickaxeItem".into(),
                "public class AdvancedPickaxeItem extends Item {
   public AdvancedPickaxeItem(ToolMaterial material, float attackDamage, float attackSpeed, Properties properties) {
      super(properties.pickaxe(material, attackDamage, attackSpeed).fireResistant());
      this.material = material;
   }
}"
                .into(),
            ),
            (
                "a.AdvancedArmorItem".into(),
                "public class AdvancedArmorItem extends Item {
   private static Properties armorProperties(ArmorMaterial material, ArmorType armorType, Properties properties) {
      return properties.durability(armorType.getDurability(4)).enchantable(material.enchantmentValue());
   }
   public AdvancedArmorItem(ArmorMaterial material, ArmorType armorType, Properties properties) {
      super(armorProperties(material, armorType, properties).fireResistant());
   }
}"
                .into(),
            ),
            (
                "a.ModToolMaterials".into(),
                "public final class ModToolMaterials {
   public static final ToolMaterial NETHERITE_DIAMOND = material(
      ModTags.INCORRECT_FOR_NETHERITE_DIAMOND_TOOL, 3092, 5.0F, 15, ItemTags.REPAIRS_NETHERITE_ARMOR
   );
   private static ToolMaterial material(
      TagKey<Block> incorrectBlocksForDrops, int durability, float attackDamageBonus, int enchantability, TagKey<Item> repairItems
   ) {
      return new ToolMaterial(incorrectBlocksForDrops, durability, 1.0F, attackDamageBonus, enchantability, repairItems);
   }
}"
                .into(),
            ),
            (
                "a.ModArmorMaterials".into(),
                "public final class ModArmorMaterials {
   public static final ArmorMaterial NETHERITE_DIAMOND = register(37, (EnumMap<ArmorType, Integer>)Util.make(new EnumMap(ArmorType.class), attribute -> {
      attribute.put(ArmorType.BOOTS, 5);
      attribute.put(ArmorType.HELMET, 5);
   }), 15, 4.0F, 0.1F, ModTags.REPAIRS_NETHERITE_DIAMOND_ARMOR, ModEquipmentAssets.NETHERITE_DIAMOND);
   private static ArmorMaterial register(
      int durability, EnumMap<ArmorType, Integer> typeProtections, int enchantability, float toughness,
      float knockbackResistance, TagKey<Item> repairIngredient, ResourceKey<EquipmentAsset> equipmentAsset
   ) {
      Holder<SoundEvent> equipSound = SoundEvents.ARMOR_EQUIP_NETHERITE;
      return new ArmorMaterial(durability, typeProtections, enchantability, equipSound, toughness, knockbackResistance, repairIngredient, equipmentAsset);
   }
}"
                .into(),
            ),
            (
                "a.ModTags".into(),
                "public class ModTags {
   public static final TagKey<Block> INCORRECT_FOR_NETHERITE_DIAMOND_TOOL = blockTag(\"incorrect_for_netherite_diamond_tool\");
   public static final TagKey<Item> REPAIRS_NETHERITE_DIAMOND_ARMOR = itemTag(\"repairs_netherite_diamond_armor\");
}"
                .into(),
            ),
            (
                "a.ModEquipmentAssets".into(),
                "public class ModEquipmentAssets {
   public static final ResourceKey<EquipmentAsset> NETHERITE_DIAMOND = ResourceKey.create(EquipmentAssets.ROOT_ID, Identifier.fromNamespaceAndPath(\"advancednetherite\", \"netherite_diamond\"));
}"
                .into(),
            ),
        ]
    }

    fn reg(id: &str, args: &str) -> Registration {
        Registration {
            registry: "item".into(),
            id: Some(id.into()),
            via: "DeferredRegister".into(),
            class: "a.ModItemRegistry".into(),
            creates: None,
            args: args.into(),
        }
    }

    #[test]
    fn reads_tools_and_armor_through_their_constants() {
        let mut scan = ModScan::default();
        scan.registrations.push(reg(
            "netherite_diamond_pickaxe",
            "() -> ModItems.NETHERITE_DIAMOND_PICKAXE",
        ));
        scan.registrations.push(reg(
            "netherite_diamond_helmet",
            "() -> ModItems.NETHERITE_DIAMOND_HELMET",
        ));
        resolve(&sources(), &mut scan);
        let pick = &scan.registrations[0];
        assert_eq!(pick.creates.as_deref(), Some("AdvancedPickaxeItem"));
        assert!(
            pick.args.contains(
                "/* super */ properties.pickaxe(ModToolMaterials.NETHERITE_DIAMOND, 1.0F, -2.8F).fireResistant()"
            ) || pick.args.contains("/* super */ new Properties().setId(key(\"netherite_diamond_pickaxe\")).pickaxe(ModToolMaterials.NETHERITE_DIAMOND, 1.0F, -2.8F).fireResistant()"),
            "{}",
            pick.args
        );
        let helmet = &scan.registrations[1];
        assert!(helmet.args.contains("ArmorType.HELMET"), "{}", helmet.args);
        assert!(helmet.args.contains(".fireResistant()"), "{}", helmet.args);

        assert_eq!(
            scan.tool_materials,
            [ToolMaterialInfo {
                name: "ModToolMaterials.NETHERITE_DIAMOND".into(),
                incorrect: Some("incorrect_for_netherite_diamond_tool".into()),
                durability: 3092,
                speed: 1.0,
                damage_bonus: 5.0,
                enchantment: 15,
                repair: Some("minecraft:repairs_netherite_armor".into()),
            }]
        );
        let armor = &scan.armor_materials[0];
        assert_eq!(armor.name, "ModArmorMaterials.NETHERITE_DIAMOND");
        assert_eq!(armor.durability, 37);
        assert_eq!(armor.defense, [("boots".into(), 5), ("helmet".into(), 5)]);
        assert_eq!(
            armor.sound.as_deref(),
            Some("minecraft:item.armor.equip_netherite")
        );
        assert_eq!(armor.toughness, 4.0);
        assert_eq!(
            armor.repair.as_deref(),
            Some("repairs_netherite_diamond_armor")
        );
        assert_eq!(
            armor.asset.as_deref(),
            Some("advancednetherite:netherite_diamond")
        );
    }
}
