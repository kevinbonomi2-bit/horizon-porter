//! A mod's tools, weapons and armor, with the stats Java gives them: the content
//! item copies the vanilla iron one of its kind and its components are set from the
//! mod's material (`properties.pickaxe(material, 1.0F, -2.8F)`,
//! `humanoidArmor(material, ArmorType.HELMET)`), as `ToolMaterial.applyToolProperties`
//! and `ArmorMaterial.humanoidProperties` set them: durability, what it mines and how
//! fast, attack damage and speed, armor, toughness, knockback resistance, the slot it
//! is worn in and the equipment asset it is drawn with, enchantability and repairs.

use porter_model::{ArmorMaterialInfo, ModScan, ToolMaterialInfo};
use serde_json::{Value, json};

/// The template and components of the item `args` (its registration, with its
/// class's `super(..)`) builds, when it is a tool, a weapon or armor.
pub(crate) fn equipment(scan: &ModScan, ns: &str, args: &str) -> Option<(String, Value)> {
    let fire = args.contains("fireResistant()");
    for kind in ["pickaxe", "axe", "shovel", "hoe", "sword", "spear"] {
        let Some(call) = builder_call(args, kind) else {
            continue;
        };
        let parts = split(&call);
        let material = tool_material(scan, parts.first()?)?;
        let nums: Vec<f32> = parts[1..].iter().filter_map(|p| number(p)).collect();
        let mut c = serde_json::Map::new();
        c.insert("minecraft:max_damage".into(), material.durability.into());
        c.insert(
            "minecraft:enchantable".into(),
            json!({ "value": material.enchantment }),
        );
        if let Some(repair) = &material.repair {
            c.insert(
                "minecraft:repairable".into(),
                json!({ "items": format!("#{}", id(ns, repair)) }),
            );
        }
        match kind {
            "spear" => {
                // spear(material, attackDuration, damageMultiplier, delay, dismountTime,
                // dismountThreshold, knockbackTime, knockbackThreshold, damageTime,
                // damageThreshold)
                let [
                    duration,
                    multiplier,
                    delay,
                    dismount_t,
                    dismount_s,
                    knock_t,
                    knock_s,
                    damage_t,
                    damage_s,
                ] = nums.get(..9)?
                else {
                    return None;
                };
                c.insert(
                    "minecraft:attribute_modifiers".into(),
                    attack(material.damage_bonus, 1.0 / duration - 4.0),
                );
                let ticks = |s: &f32| (s * 20.0).round() as i64;
                c.insert(
                    "minecraft:kinetic_weapon".into(),
                    json!({
                        "damage_conditions": { "max_duration_ticks": ticks(damage_t), "min_relative_speed": damage_s },
                        "damage_multiplier": multiplier,
                        "delay_ticks": ticks(delay),
                        "dismount_conditions": { "max_duration_ticks": ticks(dismount_t), "min_speed": dismount_s },
                        "forward_movement": 0.38,
                        "hit_sound": "minecraft:item.spear.hit",
                        "knockback_conditions": { "max_duration_ticks": ticks(knock_t), "min_speed": knock_s },
                        "sound": "minecraft:item.spear.use",
                    }),
                );
            }
            _ => {
                let [damage, speed] = nums.get(..2)? else {
                    return None;
                };
                c.insert(
                    "minecraft:attribute_modifiers".into(),
                    attack(damage + material.damage_bonus, *speed),
                );
                // A sword keeps its template's (cobwebs, bamboo); a tool mines its
                // blocks at its material's speed, and harvests all but the ones its
                // material is too weak for.
                if kind != "sword" {
                    let mut rules = Vec::new();
                    if let Some(incorrect) = &material.incorrect {
                        rules.push(json!({
                            "blocks": format!("#{}", id(ns, incorrect)),
                            "correct_for_drops": false,
                        }));
                    }
                    rules.push(json!({
                        "blocks": format!("#minecraft:mineable/{kind}"),
                        "correct_for_drops": true,
                        "speed": material.speed,
                    }));
                    c.insert("minecraft:tool".into(), json!({ "rules": rules }));
                }
            }
        }
        fire_resistance(&mut c, fire);
        return Some((format!("minecraft:iron_{kind}"), Value::Object(c)));
    }
    // Armor: `humanoidArmor(material, ArmorType.HELMET)`, or a class of the mod's
    // that takes the two.
    let piece = ["HELMET", "CHESTPLATE", "LEGGINGS", "BOOTS"]
        .into_iter()
        .find(|p| args.contains(&format!("ArmorType.{p}")))?
        .to_ascii_lowercase();
    let material = scan
        .armor_materials
        .iter()
        .find(|m| args.contains(&m.name))?;
    let (unit, slot) = match piece.as_str() {
        "helmet" => (11, "head"),
        "chestplate" => (16, "chest"),
        "leggings" => (15, "legs"),
        _ => (13, "feet"),
    };
    let mut c = serde_json::Map::new();
    c.insert(
        "minecraft:max_damage".into(),
        (unit * material.durability).into(),
    );
    c.insert(
        "minecraft:attribute_modifiers".into(),
        armor(material, &piece, slot),
    );
    let mut equippable = json!({
        "slot": slot,
        "equip_sound": material.sound.clone().unwrap_or_else(|| "minecraft:item.armor.equip_generic".into()),
    });
    if let Some(asset) = &material.asset {
        equippable["asset_id"] = id(ns, asset).into();
    }
    c.insert("minecraft:equippable".into(), equippable);
    c.insert(
        "minecraft:enchantable".into(),
        json!({ "value": material.enchantment }),
    );
    if let Some(repair) = &material.repair {
        c.insert(
            "minecraft:repairable".into(),
            json!({ "items": format!("#{}", id(ns, repair)) }),
        );
    }
    fire_resistance(&mut c, fire);
    Some((format!("minecraft:iron_{piece}"), Value::Object(c)))
}

/// `Item.Properties.fireResistant()`: it doesn't burn (the iron template does).
fn fire_resistance(c: &mut serde_json::Map<String, Value>, fire: bool) {
    if fire {
        c.insert(
            "minecraft:damage_resistant".into(),
            json!({ "types": "#minecraft:is_fire" }),
        );
    }
}

/// The main hand's attack damage and speed, as vanilla's tools add them.
fn attack(damage: f32, speed: f32) -> Value {
    json!([
        {
            "type": "minecraft:attack_damage",
            "amount": damage,
            "id": "minecraft:base_attack_damage",
            "operation": "add_value",
            "slot": "mainhand",
        },
        {
            "type": "minecraft:attack_speed",
            "amount": speed,
            "id": "minecraft:base_attack_speed",
            "operation": "add_value",
            "slot": "mainhand",
        },
    ])
}

/// A piece's armor, toughness and knockback resistance (`ArmorMaterial.createAttributes`).
fn armor(material: &ArmorMaterialInfo, piece: &str, slot: &str) -> Value {
    let defense = material
        .defense
        .iter()
        .find(|(p, _)| p == piece)
        .map_or(0, |(_, d)| *d);
    let modifier = |kind: &str, amount: f32| {
        json!({
            "type": kind,
            "amount": amount,
            "id": format!("minecraft:armor.{piece}"),
            "operation": "add_value",
            "slot": slot,
        })
    };
    let mut list = vec![
        modifier("minecraft:armor", defense as f32),
        modifier("minecraft:armor_toughness", material.toughness),
    ];
    if material.knockback > 0.0 {
        list.push(modifier(
            "minecraft:knockback_resistance",
            material.knockback,
        ));
    }
    Value::Array(list)
}

/// `ns:path`, with the mod's own namespace when `id` has none.
fn id(ns: &str, id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("{ns}:{id}")
    }
}

/// The arguments of the item builder `.pickaxe(..)` (or `pickaxe(..)` first in a
/// chain) in `args`.
fn builder_call(args: &str, kind: &str) -> Option<String> {
    let needle = format!(".{kind}(");
    let at = args.find(&needle)? + needle.len() - 1;
    let rest = &args[at..];
    let mut depth = 0;
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(rest[1..i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// A mod material by the expression naming it, else vanilla's (`ToolMaterial.DIAMOND`).
fn tool_material(scan: &ModScan, expr: &str) -> Option<ToolMaterialInfo> {
    let expr = expr.trim();
    if let Some(m) = scan
        .tool_materials
        .iter()
        .find(|m| m.name == expr || expr.ends_with(&format!(".{}", m.name)))
    {
        return Some(m.clone());
    }
    // ToolMaterial's own constants (26.3).
    let (name, durability, speed, bonus, enchantment) = match expr.strip_prefix("ToolMaterial.")? {
        "WOOD" => ("wooden", 59, 2.0, 0.0, 15),
        "STONE" => ("stone", 131, 4.0, 1.0, 5),
        "COPPER" => ("copper", 190, 5.0, 1.0, 13),
        "IRON" => ("iron", 250, 6.0, 2.0, 14),
        "DIAMOND" => ("diamond", 1561, 8.0, 3.0, 10),
        "GOLD" => ("gold", 32, 12.0, 0.0, 22),
        "NETHERITE" => ("netherite", 2031, 9.0, 4.0, 15),
        _ => return None,
    };
    let tag = if name == "gold" { "golden" } else { name };
    Some(ToolMaterialInfo {
        name: expr.to_string(),
        incorrect: Some(format!("minecraft:incorrect_for_{tag}_tool")),
        durability,
        speed,
        damage_bonus: bonus,
        enchantment,
        repair: Some(format!("minecraft:{tag}_tool_materials")),
    })
}

fn split(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in text.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
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

fn number(text: &str) -> Option<f32> {
    text.trim()
        .trim_end_matches(['f', 'F', 'd', 'D'])
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan() -> ModScan {
        let mut scan = ModScan::default();
        scan.tool_materials.push(ToolMaterialInfo {
            name: "ModToolMaterials.NETHERITE_DIAMOND".into(),
            incorrect: Some("incorrect_for_netherite_diamond_tool".into()),
            durability: 3092,
            speed: 1.0,
            damage_bonus: 5.0,
            enchantment: 15,
            repair: Some("repairs_netherite_diamond_tools".into()),
        });
        scan.armor_materials.push(ArmorMaterialInfo {
            name: "ModArmorMaterials.NETHERITE_DIAMOND".into(),
            durability: 37,
            defense: vec![("helmet".into(), 5), ("chestplate".into(), 9)],
            enchantment: 15,
            sound: Some("minecraft:item.armor.equip_netherite".into()),
            toughness: 4.0,
            knockback: 0.1,
            repair: Some("repairs_netherite_diamond_armor".into()),
            asset: Some("advancednetherite:netherite_diamond".into()),
        });
        scan
    }

    #[test]
    fn a_pickaxe_gets_its_materials_stats() {
        let args = "new AdvancedPickaxeItem(ModToolMaterials.NETHERITE_DIAMOND, 1.0F, -2.8F, new Properties())\n/* super */ new Properties().pickaxe(ModToolMaterials.NETHERITE_DIAMOND, 1.0F, -2.8F).fireResistant()";
        let (template, c) = equipment(&scan(), "advancednetherite", args).unwrap();
        assert_eq!(template, "minecraft:iron_pickaxe");
        assert_eq!(c["minecraft:max_damage"], 3092);
        assert_eq!(c["minecraft:attribute_modifiers"][0]["amount"], 6.0);
        assert_eq!(
            c["minecraft:tool"]["rules"][0]["blocks"],
            "#advancednetherite:incorrect_for_netherite_diamond_tool"
        );
        assert_eq!(
            c["minecraft:tool"]["rules"][1]["blocks"],
            "#minecraft:mineable/pickaxe"
        );
        assert_eq!(
            c["minecraft:damage_resistant"]["types"],
            "#minecraft:is_fire"
        );
        // `.axe(` isn't `.pickaxe(`'s tail.
        assert_eq!(builder_call(args, "axe"), None);
    }

    #[test]
    fn a_vanilla_material_sword() {
        let (template, c) = equipment(
            &scan(),
            "m",
            "new Item(new Item.Properties().sword(ToolMaterial.DIAMOND, 3.0F, -2.4F))",
        )
        .unwrap();
        assert_eq!(template, "minecraft:iron_sword");
        assert_eq!(c["minecraft:max_damage"], 1561);
        assert_eq!(c["minecraft:attribute_modifiers"][0]["amount"], 6.0);
        assert!(c.get("minecraft:tool").is_none());
    }

    #[test]
    fn armor_is_worn_and_drawn_with_its_asset() {
        let args = "new AdvancedArmorItem(ModArmorMaterials.NETHERITE_DIAMOND, ArmorType.CHESTPLATE, new Properties())";
        let (template, c) = equipment(&scan(), "advancednetherite", args).unwrap();
        assert_eq!(template, "minecraft:iron_chestplate");
        assert_eq!(c["minecraft:max_damage"], 16 * 37);
        assert_eq!(c["minecraft:equippable"]["slot"], "chest");
        assert_eq!(
            c["minecraft:equippable"]["asset_id"],
            "advancednetherite:netherite_diamond"
        );
        assert_eq!(c["minecraft:attribute_modifiers"][0]["amount"], 9.0);
        assert_eq!(c["minecraft:attribute_modifiers"][1]["amount"], 4.0);
        assert!(c.get("minecraft:damage_resistant").is_none());
    }
}
