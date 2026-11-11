//! What a mod does when the game tells it something happened to a player, away from its
//! blocks and items. For now one thing: a dying player's drops (NeoForge's
//! `LivingDropsEvent`), which grave mods are built on. The Java becomes a function in
//! runtime.rs, as a block's behaviours do; the API glue calls it from `living_drops`.
//! Horizon's own hooks tell a mod that a player died, not what they drop, so a
//! horizon-mixins bundle has nothing to call it from yet.

use porter_model::ModScan;

/// The mod's code for a dying player's drops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drops {
    /// The classes it is in: those that hear `LivingDropsEvent`, and those that hear
    /// an event of the mod's own posted from there (a library's `PlayerDeathEvent`).
    pub classes: Vec<String>,
}

/// The classes of `scan` that handle a dying player's drops, if any do.
pub fn drops(scan: &ModScan) -> Option<Drops> {
    let mut classes: Vec<String> = Vec::new();
    for class in scan.classes.iter().filter(|c| {
        !c.is_mixin
            && c.references
                .iter()
                .any(|r| r.rsplit('.').next() == Some("LivingDropsEvent"))
    }) {
        if !classes.contains(&class.name) {
            classes.push(class.name.clone());
        }
        // An event of the mod's own it posts from there: who hears that one.
        for event in class.references.iter().filter(|r| {
            r.rsplit('.').next().is_some_and(|s| s.ends_with("Event"))
                && scan.classes.iter().any(|k| k.name == **r)
        }) {
            for other in scan
                .classes
                .iter()
                .filter(|o| !o.is_mixin && o.name != *event && o.references.contains(event))
            {
                if !classes.contains(&other.name) {
                    classes.push(other.name.clone());
                }
            }
        }
    }
    classes.truncate(4);
    (!classes.is_empty()).then_some(Drops { classes })
}

/// runtime.rs's part: what a death hands the code, and the function to port.
pub fn runtime(drops: &Drops) -> String {
    let from = drops.classes.join(" and ");
    format!(
        r#"
/// A player died and their items are about to drop (NeoForge's `LivingDropsEvent` for a
/// player, what a library's `PlayerDeathEvent` is posted from).
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerDrops {{
    /// The player's name and UUID (`player.getName().getString()`,
    /// `player.getUUID().toString()`).
    pub name: String,
    pub uuid: String,
    /// The block they died in (`player.blockPosition()`), and the way they faced, flat
    /// (`player.getDirection()`): `north`.
    pub pos: [i32; 3],
    pub facing: String,
    /// What is about to drop (the event's `getDrops()`): each stack's item and count, in
    /// order. Empty under the `keepInventory` game rule.
    pub items: Vec<(String, u32)>,
}}

/// The mod's code for a dying player's drops. True when it took them (the Java clears
/// the event's drops, cancels it, or asks its library to `removeDrops()`): nothing
/// drops then.
pub fn player_drops(d: &PlayerDrops, world: &mut dyn World) -> bool {{
    on_player_drops(d, world)
}}

/// What the mod does with a dying player's drops: follow the Java from the handler of
/// `LivingDropsEvent` through any event of its own it posts to the code that hears it.
/// Change blocks through `world` (`world.set_block`), and keep what it stores in a block
/// entity there: a stack in each of its item slots (`world.set_entity_item(pos, slot,
/// Some((item, count)))`, as many as `world.entity_slots(pos)`), its plain values with
/// `world.save_entity`. A message to the player is `world.chat`. Return true where the
/// Java removes the drops, false where it leaves them (no room for a grave,
/// `keepInventory`).
///
/// TODO: port from {from}.
fn on_player_drops(d: &PlayerDrops, world: &mut dyn World) -> bool {{
    let _ = (d, world);
    false
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_function_to_port() {
        let text = runtime(&Drops {
            classes: vec!["a.lib.DeathEvents".into(), "a.events.DeathEvents".into()],
        });
        assert!(text.contains("TODO: port from a.lib.DeathEvents and a.events.DeathEvents."));
        assert!(
            text.contains("pub fn player_drops(d: &PlayerDrops, world: &mut dyn World) -> bool")
        );
    }
}
