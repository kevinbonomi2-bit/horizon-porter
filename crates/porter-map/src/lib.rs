//! The mapping database: what a Java mod's API use becomes in Horizon.
//!
//! [`client_hooks`] maps Fabric and NeoForge client events and callbacks to the
//! Horizon hook points (from Horizon's `native-hooks.json`) that do the same job.
//! [`hooks`] is the table of every hook point, with its Rust signature, and maps
//! Mixin targets and loader events to hooks.

pub mod hooks;
pub mod ledger;
pub mod reach;

/// One Java API, and the Horizon hooks that take its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookMapping {
    /// Fully qualified Java class: an event, a callback or a registry helper.
    pub java: &'static str,
    /// Horizon hook names, as `native-hooks.json` lists them (`native_<name>` in code).
    pub hooks: &'static [&'static str],
    /// What the port does with it.
    pub note: &'static str,
}

const fn m(java: &'static str, hooks: &'static [&'static str], note: &'static str) -> HookMapping {
    HookMapping { java, hooks, note }
}

/// Fabric API and NeoForge client APIs, and their Horizon hooks.
pub const CLIENT_HOOKS: &[HookMapping] = &[
    // Fabric API.
    m(
        "net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents",
        &["tick"],
        "client tick listener",
    ),
    m(
        "net.fabricmc.fabric.api.client.rendering.v1.HudRenderCallback",
        &["hud"],
        "HUD drawing",
    ),
    m(
        "net.fabricmc.fabric.api.client.rendering.v1.hud.HudElementRegistry",
        &["hud"],
        "HUD element",
    ),
    m(
        "net.fabricmc.fabric.api.client.rendering.v1.HudLayerRegistrationCallback",
        &["hud"],
        "HUD layer",
    ),
    m(
        "net.fabricmc.fabric.api.client.keybinding.v1.KeyBindingHelper",
        &["key", "mods"],
        "key binding: listed in Options > Mods, rebindable",
    ),
    m(
        "net.fabricmc.fabric.api.client.command.v2.ClientCommandRegistrationCallback",
        &["command"],
        "client command",
    ),
    m(
        "net.fabricmc.fabric.api.client.command.v2.ClientCommandManager",
        &["command"],
        "client command",
    ),
    m(
        "net.fabricmc.fabric.api.client.item.v1.ItemTooltipCallback",
        &["tooltip"],
        "item tooltip lines",
    ),
    m(
        "net.fabricmc.fabric.api.client.screen.v1.ScreenEvents",
        &["screen_init_post", "screen_render_post", "screen_closing"],
        "screen events",
    ),
    m(
        "net.fabricmc.fabric.api.client.screen.v1.ScreenKeyboardEvents",
        &["screen_key_pressed_pre", "screen_key_released_pre"],
        "screen key events",
    ),
    m(
        "net.fabricmc.fabric.api.client.screen.v1.ScreenMouseEvents",
        &[
            "screen_mouse_button_pressed_pre",
            "screen_mouse_scrolled_pre",
        ],
        "screen mouse events",
    ),
    m(
        "net.fabricmc.fabric.api.client.message.v1.ClientReceiveMessageEvents",
        &["client_chat_received"],
        "incoming chat",
    ),
    m(
        "net.fabricmc.fabric.api.client.rendering.v1.WorldRenderEvents",
        &["world_drawn"],
        "world rendering",
    ),
    m(
        "net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents",
        &["player_client_loaded_world", "client_level_unload"],
        "joining and leaving a world",
    ),
    m(
        "com.terraformersmc.modmenu.api.ModMenuApi",
        &["settings", "settings_click"],
        "config screen: becomes the mod's page in Options > Mods",
    ),
    // NeoForge.
    m(
        "net.neoforged.neoforge.client.event.ClientTickEvent",
        &["tick"],
        "client tick listener",
    ),
    m(
        "net.neoforged.neoforge.client.event.RenderGuiEvent",
        &["hud"],
        "HUD drawing",
    ),
    m(
        "net.neoforged.neoforge.client.event.RenderGuiLayerEvent",
        &["gui_layer_pre", "gui_layer_post"],
        "around a vanilla HUD layer",
    ),
    m(
        "net.neoforged.neoforge.client.event.RegisterGuiLayersEvent",
        &["hud"],
        "HUD layer",
    ),
    m(
        "net.neoforged.neoforge.client.event.RegisterKeyMappingsEvent",
        &["key", "mods"],
        "key binding: listed in Options > Mods, rebindable",
    ),
    m(
        "net.neoforged.neoforge.client.event.InputEvent.Key",
        &["key"],
        "key presses",
    ),
    m(
        "net.neoforged.neoforge.client.event.InputEvent.MouseScrollingEvent",
        &["wheel"],
        "mouse wheel",
    ),
    m(
        "net.neoforged.neoforge.client.event.RegisterClientCommandsEvent",
        &["command"],
        "client command",
    ),
    m(
        "net.neoforged.neoforge.client.event.ScreenEvent",
        &[
            "screen_opening",
            "screen_init_post",
            "screen_render_post",
            "screen_closing",
        ],
        "screen events",
    ),
    // Outer classes, for imports that name only them (`ViewportEvent`, then
    // `ViewportEvent.ComputeFov` in a parameter): the nested entries are more precise.
    m(
        "net.neoforged.neoforge.client.event.InputEvent",
        &["key", "wheel"],
        "input events",
    ),
    m(
        "net.neoforged.neoforge.client.event.ViewportEvent",
        &["compute_fov", "render_fog", "camera_pose"],
        "viewport events",
    ),
    m(
        "net.neoforged.neoforge.client.event.ViewportEvent.ComputeFov",
        &["compute_fov"],
        "field of view",
    ),
    m(
        "net.neoforged.neoforge.client.event.ComputeFovModifierEvent",
        &["compute_fov_modifier"],
        "field of view modifier",
    ),
    m(
        "net.neoforged.neoforge.client.event.ViewportEvent.RenderFog",
        &["render_fog"],
        "fog",
    ),
    m(
        "net.neoforged.neoforge.client.event.ViewportEvent.ComputeCameraAngles",
        &["camera_pose"],
        "camera angles",
    ),
    m(
        "net.neoforged.neoforge.client.event.ClientChatReceivedEvent",
        &["client_chat_received"],
        "incoming chat",
    ),
    m(
        "net.neoforged.neoforge.client.event.RenderLevelStageEvent",
        &["world_drawn"],
        "world rendering",
    ),
    m(
        "net.neoforged.neoforge.client.event.RenderHandEvent",
        &["render_hand"],
        "first-person hand",
    ),
    m(
        "net.neoforged.neoforge.client.event.RenderNameTagEvent",
        &["render_name_tag"],
        "name tags",
    ),
    m(
        "net.neoforged.neoforge.client.event.RenderTooltipEvent",
        &["tooltip_color", "tooltip_component"],
        "tooltip drawing",
    ),
    m(
        "net.neoforged.neoforge.event.entity.player.ItemTooltipEvent",
        &["tooltip"],
        "item tooltip lines",
    ),
    m(
        "net.neoforged.neoforge.client.event.ClientPlayerNetworkEvent",
        &["player_client_loaded_world", "client_level_unload"],
        "joining and leaving a world",
    ),
    m(
        "net.neoforged.neoforge.client.gui.IConfigScreenFactory",
        &["settings", "settings_click"],
        "config screen: becomes the mod's page in Options > Mods",
    ),
];

/// The mapping for a class the mod refers to: the class itself or one nested in it
/// (`ScreenEvent.Opening` matches `ScreenEvent`). The most specific entry wins.
pub fn client_hooks(fqcn: &str) -> Option<&'static HookMapping> {
    CLIENT_HOOKS
        .iter()
        .filter(|m| {
            fqcn == m.java
                || fqcn
                    .strip_prefix(m.java)
                    .is_some_and(|rest| rest.starts_with('.'))
        })
        .max_by_key(|m| m.java.len())
}

/// Hooks whose signatures the porter knows, so it can wire them completely.
pub const WIRED_HOOKS: &[&str] = &[
    "mods",
    "key",
    "command",
    "hud",
    "tick",
    "wheel",
    "settings",
    "settings_click",
];

/// A GLFW key constant (`GLFW_KEY_Z`, `GLFW.GLFW_KEY_LEFT_SHIFT`,
/// `GLFW_MOUSE_BUTTON_MIDDLE`) in vanilla's key naming, as Horizon uses it:
/// `key.keyboard.z`, `key.keyboard.left.shift`, `key.mouse.middle`.
pub fn glfw_key_name(constant: &str) -> Option<String> {
    let name = constant.rsplit('.').next()?;
    if let Some(key) = name.strip_prefix("GLFW_KEY_") {
        let key = key.to_ascii_lowercase();
        let key = match key.strip_prefix("kp_") {
            Some(rest) => format!("keypad.{rest}"),
            None => key,
        };
        return Some(format!("key.keyboard.{}", key.replace('_', ".")));
    }
    if let Some(button) = name.strip_prefix("GLFW_MOUSE_BUTTON_") {
        return Some(format!("key.mouse.{}", button.to_ascii_lowercase()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_events_and_nested_events() {
        assert_eq!(
            client_hooks("net.neoforged.neoforge.client.event.ClientTickEvent.Post")
                .unwrap()
                .hooks,
            &["tick"]
        );
        // The nested ViewportEvent.ComputeFov is more specific than any ViewportEvent entry.
        assert_eq!(
            client_hooks("net.neoforged.neoforge.client.event.ViewportEvent.ComputeFov")
                .unwrap()
                .hooks,
            &["compute_fov"]
        );
        assert!(client_hooks("net.neoforged.neoforge.client.event.ClientTickEventX").is_none());
        assert!(client_hooks("net.minecraft.world.level.Level").is_none());
    }

    #[test]
    fn names_glfw_keys() {
        assert_eq!(
            glfw_key_name("GLFW_KEY_Z").as_deref(),
            Some("key.keyboard.z")
        );
        assert_eq!(
            glfw_key_name("GLFW.GLFW_KEY_LEFT_SHIFT").as_deref(),
            Some("key.keyboard.left.shift")
        );
        assert_eq!(
            glfw_key_name("GLFW_KEY_KP_0").as_deref(),
            Some("key.keyboard.keypad.0")
        );
        assert_eq!(
            glfw_key_name("GLFW_MOUSE_BUTTON_MIDDLE").as_deref(),
            Some("key.mouse.middle")
        );
        assert_eq!(glfw_key_name("SOMETHING"), None);
    }
}
