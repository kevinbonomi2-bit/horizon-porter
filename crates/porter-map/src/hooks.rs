//! Horizon's native hook points, as `tools/horizon_hooks.py` extracts them from
//! Horizon's source (MIT, Copyright (c) 2026 Ludovic Marie and the Horizon
//! contributors): each hook's target, contract, Rust signature and the vanilla
//! methods and loader events its doc names. The porter wires any of them from this,
//! and maps Mixins and events to them.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde::Deserialize;

/// One hook point.
#[derive(Debug, Clone, Deserialize)]
pub struct HorizonHook {
    /// As `native-hooks.json` names it: `hud` for `native_hud`.
    pub name: String,
    pub version: u32,
    /// The hooked function: `horizon_client::game::native_hooks::Game<'_>::native_hud`.
    pub target: String,
    /// `listen`, `cancel`, `replace` or `collect`.
    pub contract: Option<String>,
    /// The parameter a `replace` or `collect` hook fills in.
    pub out: Option<String>,
    /// The hook points version it came in, when its doc says.
    pub api: Option<u32>,
    /// The function's generic parameters, as written (`<'a>`).
    pub generics: String,
    /// The receiver and parameters, as written: `&mut self`, `batch: &mut HudBatch`.
    pub params: Vec<String>,
    /// The return type, empty for none.
    pub returns: String,
    /// The first sentence of its doc.
    pub doc: String,
    /// Vanilla methods and loader events its doc names: `Screen.render`,
    /// `LivingEvent.LivingJumpEvent`.
    pub cites: Vec<String>,
}

/// The hook table and the Horizon commit it was read from.
#[derive(Debug, Deserialize)]
pub struct Catalog {
    pub horizon: String,
    pub hooks: Vec<HorizonHook>,
}

/// The hook points of the Horizon commit in `data/horizon-hooks.json`.
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../data/horizon-hooks.json"))
            .expect("data/horizon-hooks.json is valid")
    })
}

/// The hook named `name` (`hud`), if Horizon has it.
pub fn hook(name: &str) -> Option<&'static HorizonHook> {
    catalog().hooks.iter().find(|h| h.name == name)
}

/// One parameter of a hook's helper: its name, type and the value the hooked
/// function passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub name: String,
    pub ty: String,
    pub value: String,
}

impl HorizonHook {
    /// The hook points version a bundle that uses it needs.
    pub fn needs(&self) -> u32 {
        self.api.unwrap_or(1)
    }

    /// The helper's parameters, from the hooked function's: the receiver as
    /// `game`, references passed on (reborrowed when mutable), plain values copied,
    /// and other values by reference, so the hooked function keeps them. None when
    /// the signature has what a helper can't take (type parameters, a return value).
    pub fn bindings(&self) -> Option<Vec<Binding>> {
        if !self.returns.is_empty() {
            return None;
        }
        // Named lifetimes become elided ones; type parameters can't be named.
        let lifetimes: Vec<&str> = self
            .generics
            .trim_start_matches('<')
            .trim_end_matches('>')
            .split(',')
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(|g| g.split(':').next().unwrap_or(g).trim())
            .collect();
        if lifetimes.iter().any(|g| !g.starts_with('\'')) {
            return None;
        }
        let elide = |ty: &str| {
            let mut ty = ty.to_string();
            for lt in &lifetimes {
                ty = replace_lifetime(&ty, lt);
            }
            ty
        };
        let mut out = Vec::new();
        for param in &self.params {
            let param = param.trim();
            if param == "&mut self" || param == "&self" || param == "self" || param == "mut self" {
                let ty = self.receiver_type()?;
                let (ty, value) = if param == "&mut self" {
                    (format!("&mut {ty}"), "&mut *self".to_string())
                } else {
                    (format!("&{ty}"), "&*self".to_string())
                };
                out.push(Binding {
                    name: "game".into(),
                    ty,
                    value,
                });
                continue;
            }
            let (name, ty) = param.split_once(':')?;
            let name = name.trim().trim_start_matches("mut ").trim().to_string();
            if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None; // a pattern, not a name
            }
            let ty = elide(ty.trim());
            let binding = if let Some(inner) = ty.strip_prefix("&mut ") {
                Binding {
                    value: format!("&mut *{name}"),
                    ty: format!("&mut {inner}"),
                    name,
                }
            } else if ty.starts_with('&') || is_copy(&ty) {
                Binding {
                    value: name.clone(),
                    ty,
                    name,
                }
            } else if ty.starts_with("impl ") {
                return None;
            } else {
                Binding {
                    value: format!("&{name}"),
                    ty: format!("&{ty}"),
                    name,
                }
            };
            out.push(binding);
        }
        Some(out)
    }

    /// The receiver's type, from the target path: `crate::game::Game<'_>`.
    fn receiver_type(&self) -> Option<String> {
        let (path, _) = self.target.rsplit_once("::")?;
        let ty = path.rsplit("::").next()?;
        if !ty.starts_with(|c: char| c.is_ascii_uppercase()) {
            return None;
        }
        // Helpers are compiled in the hooked function's crate. `Game` is defined
        // in `game` (native_hooks has an `impl` of it): name it there.
        let (_, rest) = path.split_once("::")?;
        if rest == "game::native_hooks::Game<'_>" {
            return Some("crate::game::Game<'_>".into());
        }
        Some(format!("crate::{rest}"))
    }
}

/// `ty` with the named lifetime `lt` (`'a`) elided to `'_`.
fn replace_lifetime(ty: &str, lt: &str) -> String {
    let mut out = String::new();
    let mut rest = ty;
    while let Some(i) = rest.find(lt) {
        let after = rest[i + lt.len()..].chars().next();
        out.push_str(&rest[..i]);
        if after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            out.push_str(lt);
        } else {
            out.push_str("'_");
        }
        rest = &rest[i + lt.len()..];
    }
    out.push_str(rest);
    out
}

/// Types passed by value without moving anything: numbers, `bool`, `char`, and
/// arrays and tuples of them.
fn is_copy(ty: &str) -> bool {
    const PLAIN: &[&str] = &[
        "bool", "char", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16",
        "u32", "u64", "u128", "usize",
    ];
    let ty = ty.trim();
    if PLAIN.contains(&ty) {
        return true;
    }
    if let Some(inner) = ty.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        return inner.split(';').next().is_some_and(is_copy);
    }
    if let Some(inner) = ty.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        return inner.split(',').all(|t| t.trim().is_empty() || is_copy(t));
    }
    false
}

/// Yarn class names (Fabric mods before 26.1) and their Mojang names, which
/// Horizon's docs use. Only classes Mixins commonly target.
const YARN_CLASSES: &[(&str, &str)] = &[
    ("MinecraftClient", "Minecraft"),
    ("InGameHud", "Gui"),
    ("WorldRenderer", "LevelRenderer"),
    ("ClientPlayerEntity", "LocalPlayer"),
    ("PlayerEntity", "Player"),
    ("ServerPlayerEntity", "ServerPlayer"),
    ("World", "Level"),
    ("ServerWorld", "ServerLevel"),
    ("ClientWorld", "ClientLevel"),
    ("HandledScreen", "AbstractContainerScreen"),
    ("Keyboard", "KeyboardHandler"),
    ("Mouse", "MouseHandler"),
    ("ChatHud", "ChatComponent"),
    ("AbstractBlock", "BlockBehaviour"),
    ("ServerPlayNetworkHandler", "ServerGamePacketListenerImpl"),
    ("ClientPlayNetworkHandler", "ClientPacketListener"),
    ("ServerPlayerInteractionManager", "ServerPlayerGameMode"),
    ("ClientPlayerInteractionManager", "MultiPlayerGameMode"),
    ("BackgroundRenderer", "FogRenderer"),
    ("HeldItemRenderer", "ItemInHandRenderer"),
    ("ScreenHandler", "AbstractContainerMenu"),
];

/// Common Mixin targets on the client and the hooks that do their job, for the
/// vanilla methods Horizon's hook docs don't name.
const MIXIN_TARGETS: &[(&str, &[&str])] = &[
    ("Gui.render", &["hud"]),
    ("Gui.renderHotbar", &["hud"]),
    ("Minecraft.tick", &["tick"]),
    ("Minecraft.runTick", &["frame"]),
    ("GameRenderer.getFov", &["compute_fov"]),
    ("GameRenderer.render", &["frame"]),
    ("GameRenderer.renderLevel", &["world_drawn"]),
    ("GameRenderer.renderItemInHand", &["render_hand"]),
    ("ItemInHandRenderer.renderHandsWithItems", &["render_hand"]),
    ("LevelRenderer.renderLevel", &["world_drawn"]),
    ("FogRenderer.setupFog", &["render_fog"]),
    ("FogRenderer.computeFogColor", &["compute_fog_color"]),
    ("KeyboardHandler.keyPress", &["key", "input_key"]),
    ("MouseHandler.onScroll", &["wheel", "mouse_scrolling"]),
    ("MouseHandler.onPress", &["mouse_button_pre"]),
    ("Screen.render", &["screen_render_post"]),
    ("Screen.renderWithTooltip", &["screen_render_post"]),
    ("Minecraft.setScreen", &["screen"]),
    ("ItemStack.getTooltipLines", &["tooltip"]),
    ("ClientPacketListener.sendCommand", &["command"]),
    ("ClientPacketListener.sendUnsignedCommand", &["command"]),
    ("Camera.setup", &["camera_distance"]),
    ("LocalPlayer.tick", &["player_tick"]),
    ("Player.tick", &["player_tick"]),
    ("Minecraft.setLevel", &["client_level_load"]),
    ("Options.load", &["settings"]),
];

/// The class name a doc would cite: the last part of a qualified name, nested
/// classes joined with a dot, Yarn names in Mojang's.
pub fn doc_class(fqcn: &str) -> String {
    let simple = fqcn.rsplit('.').next().unwrap_or(fqcn).replace('$', ".");
    let (outer, inner) = simple.split_once('.').unwrap_or((&simple, ""));
    let outer = YARN_CLASSES
        .iter()
        .find(|(yarn, _)| *yarn == outer)
        .map_or(outer, |(_, mojang)| mojang);
    if inner.is_empty() {
        outer.to_string()
    } else {
        format!("{outer}.{inner}")
    }
}

/// How a Java method was matched to a hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Match {
    /// The hook's doc names the method (`Screen.render`).
    Cited,
    /// The porter's own table of common client targets.
    Known,
    /// The hook is named for the method (`canEat` → `can_eat`).
    Named,
}

impl Match {
    pub fn describe(self) -> &'static str {
        match self {
            Match::Cited => "its doc names the method",
            Match::Known => "the porter's map of client targets",
            Match::Named => "named for the method: check it fits",
        }
    }
}

/// The hooks that do the job of `class.method` (a Mixin's target): `class` fully
/// qualified or simple, `method` as a Mixin writes it (a descriptor after it is
/// ignored).
pub fn method_hooks(class: &str, method: &str) -> Vec<(&'static HorizonHook, Match)> {
    let method = method.split('(').next().unwrap_or(method).trim();
    let method = method.rsplit(['.', ';']).next().unwrap_or(method);
    if method.is_empty() || method.starts_with('<') {
        return Vec::new();
    }
    let key = format!("{}.{method}", doc_class(class));
    let mut out: Vec<(&'static HorizonHook, Match)> = Vec::new();
    let mut seen = BTreeSet::new();
    for h in &catalog().hooks {
        if h.cites.contains(&key) && seen.insert(h.name.as_str()) {
            out.push((h, Match::Cited));
        }
    }
    if let Some((_, names)) = MIXIN_TARGETS.iter().find(|(k, _)| *k == key) {
        for name in *names {
            if let Some(h) = hook(name)
                && seen.insert(h.name.as_str())
            {
                out.push((h, Match::Known));
            }
        }
    }
    if out.is_empty() {
        let snake = snake_case(method);
        let bare = ["is_", "get_", "on_", "do_"]
            .iter()
            .find_map(|p| snake.strip_prefix(p))
            .unwrap_or(&snake);
        for h in &catalog().hooks {
            if (h.name == snake || h.name == bare) && seen.insert(h.name.as_str()) {
                out.push((h, Match::Named));
            }
        }
    }
    out
}

/// The hooks for a loader event class the mod uses, from the events Horizon's
/// hook docs name: `net.neoforged.neoforge.event.entity.living.LivingEvent.LivingJumpEvent`
/// matches a hook citing `LivingEvent.LivingJumpEvent`.
pub fn event_hooks(fqcn: &str) -> Vec<&'static HorizonHook> {
    if !fqcn.contains(".event") {
        return Vec::new();
    }
    let parts: Vec<&str> = fqcn.split(['.', '$']).collect();
    let n = parts.len();
    let mut keys = Vec::new();
    if n >= 2 && parts[n - 2].starts_with(|c: char| c.is_ascii_uppercase()) {
        keys.push(format!("{}.{}", parts[n - 2], parts[n - 1]));
    }
    if let Some(last) = parts.last() {
        keys.push((*last).to_string());
    }
    catalog()
        .hooks
        .iter()
        .filter(|h| h.cites.iter().any(|c| keys.contains(c)))
        .collect()
}

/// `isSmokeSource` → `is_smoke_source`.
fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_catalog() {
        let c = catalog();
        assert_eq!(c.horizon.len(), 40);
        assert!(c.hooks.len() > 300);
        let hud = hook("hud").unwrap();
        assert_eq!(hud.needs(), 1);
        let b = hud.bindings().unwrap();
        assert_eq!(
            b[0],
            Binding {
                name: "game".into(),
                ty: "&mut crate::game::Game<'_>".into(),
                value: "&mut *self".into()
            }
        );
        assert_eq!(b[1].ty, "&mut HudBatch");
        assert_eq!(b[1].value, "&mut *batch");
        // Every hook the porter wires completely is in the catalog.
        for name in crate::WIRED_HOOKS {
            assert!(hook(name).is_some(), "{name}");
        }
        // Every hook can be given a helper.
        let unbindable: Vec<&str> = c
            .hooks
            .iter()
            .filter(|h| h.bindings().is_none())
            .map(|h| h.name.as_str())
            .collect();
        assert!(unbindable.is_empty(), "{unbindable:?}");
    }

    #[test]
    fn binds_values_by_reference_unless_plain() {
        let fov = hook("compute_fov").unwrap().bindings().unwrap();
        let tys: Vec<(&str, &str)> = fov
            .iter()
            .map(|b| (b.ty.as_str(), b.value.as_str()))
            .collect();
        assert_eq!(
            tys,
            [
                ("&crate::game::Game<'_>", "&*self"),
                ("f32", "partial"),
                ("[f64; 3]", "camera"),
                ("&mut f32", "&mut *degrees"),
            ]
        );
        let changed = hook("block_changed").unwrap().bindings().unwrap();
        assert!(
            changed
                .iter()
                .any(|b| b.ty == "&BlockPos" && b.value == "&pos")
        );
        assert!(changed[0].ty.contains("GameLevel<'_>"));
        // Named lifetimes are elided.
        let drops = hook("loot_drops").unwrap().bindings().unwrap();
        assert_eq!(drops[1].ty, "&NativeLootDrops<'_>");
        assert_eq!(
            replace_lifetime("&'a Foo<'ab, 'a>", "'a"),
            "&'_ Foo<'ab, '_>"
        );
    }

    #[test]
    fn maps_mixin_targets() {
        let names = |class: &str, method: &str| {
            method_hooks(class, method)
                .into_iter()
                .map(|(h, m)| (h.name.as_str(), m))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names("net.minecraft.client.gui.Gui", "render"),
            [("hud", Match::Known)]
        );
        // Yarn's name for the same class.
        assert_eq!(
            names(
                "net.minecraft.client.gui.hud.InGameHud",
                "render(Lnet/minecraft/client/gui/DrawContext;F)V"
            ),
            [("hud", Match::Known)]
        );
        assert!(
            names("net.minecraft.world.level.Level", "setBlock")
                .iter()
                .any(|(_, m)| *m == Match::Cited)
        );
        assert_eq!(
            names("net.minecraft.world.entity.player.Player", "canEat"),
            [("can_eat", Match::Cited)]
        );
        // A hook named for the method, when no doc names it.
        assert!(
            names(
                "net.minecraft.world.level.block.CampfireBlock",
                "isSmokeSource"
            )
            .iter()
            .any(|(n, _)| *n == "smoke_source")
        );
        assert!(names("net.minecraft.world.level.block.FarmBlock", "turnToDirt").is_empty());
        assert_eq!(doc_class("a.b.Outer$Inner"), "Outer.Inner");
    }

    #[test]
    fn maps_loader_events() {
        let hooks =
            event_hooks("net.neoforged.neoforge.event.entity.living.LivingEvent.LivingJumpEvent");
        assert!(hooks.iter().any(|h| h.name == "living_jump"), "{hooks:?}");
        assert!(event_hooks("net.minecraft.world.level.Level").is_empty());
    }
}
