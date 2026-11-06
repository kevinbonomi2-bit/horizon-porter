//! What a mod's Mixins change, and whether a port can reach it.
//!
//! A Mixin verdict says whether horizon-mixins has the same *kind* of injection.
//! That is not enough to port the mod: a Mixin that changes how Minecraft draws the
//! world or a screen hooks code Horizon doesn't have, because Horizon draws with its
//! own renderer. This sorts each Mixin member by what it changes and gives the mod
//! an outlook a player can act on.

use porter_model::{MixinClass, MixinMember, ModScan, Verdict};

use crate::hooks::{self, Match};

/// What one Mixin member changes, as far as a port is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reach {
    /// A Horizon hook does the same job.
    Hook,
    /// Game logic with no hook: the port injects into Horizon's own function for
    /// it, pinned to a Horizon commit.
    Horizon,
    /// How a screen is drawn or laid out. Horizon draws its own screens; hooks let
    /// a port draw on them and handle their input, not replace them.
    Screen,
    /// How the world, entities, items or the GUI are drawn. Horizon has its own
    /// renderer, which ports can't change yet.
    Rendering,
}

impl Reach {
    pub fn describe(self) -> &'static str {
        match self {
            Reach::Hook => "a Horizon hook does the same job",
            Reach::Horizon => "game logic: ported into Horizon's own function for it",
            Reach::Screen => "how a screen is drawn: Horizon draws its own screens",
            Reach::Rendering => "how the game draws: Horizon has its own renderer",
        }
    }
}

/// Whether a mod can be ported today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Portability {
    /// Nothing it changes is out of reach.
    Likely,
    /// Some features change how the game draws and won't carry over.
    Partly,
    /// What the mod is for is out of reach.
    NotYet,
}

/// The mod's outlook: how many Mixin members fall in each [`Reach`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outlook {
    pub hook: usize,
    pub horizon: usize,
    pub screen: usize,
    pub rendering: usize,
    pub portability: Portability,
    /// One or two sentences for a player.
    pub summary: String,
}

impl Outlook {
    /// Members that change something (accessors, shadows and the like left out).
    pub fn total(&self) -> usize {
        self.hook + self.horizon + self.screen + self.rendering
    }

    /// Members out of reach today.
    pub fn out_of_reach(&self) -> usize {
        self.screen + self.rendering
    }
}

/// Members that only follow their Mixin class (they change nothing themselves).
fn follows(member: &MixinMember) -> bool {
    matches!(member.verdict, Verdict::NotNeeded(_))
        || ["Shadow", "Accessor", "Invoker", "Unique", "merged"]
            .contains(&member.annotation.as_str())
}

/// Hooks that only say when something happens; they give a port nothing to draw
/// with.
const NOTIFY_ONLY: &[&str] = &["frame", "tick"];

/// Packages and classes that draw: Mojang names (26.x) and Yarn names (older
/// Fabric mods).
const RENDERING_PACKAGES: &[&str] = &[
    "com.mojang.blaze3d.",
    "net.minecraft.client.renderer.",
    "net.minecraft.client.render.",
    "net.minecraft.client.model.",
    "net.minecraft.client.gui.render.",
    "net.minecraft.client.gui.font.",
    "net.minecraft.client.font.",
    "net.minecraft.client.particle.",
    "net.minecraft.client.resources.model.",
];

const RENDERING_CLASSES: &[&str] = &[
    "GameRenderer",
    "LevelRenderer",
    "WorldRenderer",
    "GuiGraphics",
    "DrawContext",
    "RenderSystem",
    "BufferBuilder",
    "BufferUploader",
    "Tesselator",
    "Tessellator",
    "PoseStack",
    "MatrixStack",
    "VertexConsumer",
    "ItemRenderer",
    "ItemInHandRenderer",
    "HeldItemRenderer",
    "EntityRenderDispatcher",
    "BlockEntityRenderDispatcher",
    "BlockRenderDispatcher",
    "ShaderInstance",
    "ShaderProgram",
    "RenderTarget",
    "Framebuffer",
    "Font",
    "TextRenderer",
    "LightTexture",
    "LightmapTextureManager",
    "Window",
];

const SCREEN_PACKAGES: &[&str] = &[
    "net.minecraft.client.gui.screens.",
    "net.minecraft.client.gui.screen.",
    "net.minecraft.client.gui.components.",
    "net.minecraft.client.gui.widget.",
];

const SCREEN_CLASSES: &[&str] = &[
    "Screen",
    "AbstractContainerScreen",
    "HandledScreen",
    "InventoryScreen",
    "CreativeModeInventoryScreen",
    "CreativeInventoryScreen",
    "TitleScreen",
    "PauseScreen",
    "GameMenuScreen",
    "ChatScreen",
    "AbstractWidget",
    "ClickableWidget",
    "Button",
    "ButtonWidget",
    "EditBox",
    "TextFieldWidget",
];

fn simple(class: &str) -> &str {
    let last = class.rsplit('.').next().unwrap_or(class);
    last.split('$').next().unwrap_or(last)
}

fn in_packages(class: &str, packages: &[&str], classes: &[&str]) -> bool {
    packages.iter().any(|p| class.starts_with(p)) || classes.contains(&simple(class))
}

/// What a Mixin target class draws, if it draws.
fn drawing(class: &str) -> Option<Reach> {
    if in_packages(class, RENDERING_PACKAGES, RENDERING_CLASSES)
        || simple(class).ends_with("Renderer")
    {
        Some(Reach::Rendering)
    } else if in_packages(class, SCREEN_PACKAGES, SCREEN_CLASSES)
        || simple(class).ends_with("Screen")
    {
        Some(Reach::Screen)
    } else {
        None
    }
}

/// What one Mixin member changes.
pub fn reach(mixin: &MixinClass, member: &MixinMember) -> Reach {
    let mut best: Option<Reach> = None;
    let mut keep = |r: Reach| {
        // The member reaches as far as its best target.
        best = Some(best.map_or(r, |b| b.min(r)));
    };
    for target in &mixin.targets {
        let draws = drawing(target);
        let hooked = member.target_methods.iter().any(|method| {
            hooks::method_hooks(target, method).iter().any(|(h, how)| {
                *how != Match::Named && !(draws.is_some() && NOTIFY_ONLY.contains(&h.name.as_str()))
            })
        });
        keep(if hooked {
            Reach::Hook
        } else {
            draws.unwrap_or(Reach::Horizon)
        });
    }
    best.unwrap_or(Reach::Horizon)
}

/// The outlook for a scanned mod.
pub fn outlook(scan: &ModScan) -> Outlook {
    let (mut hook, mut horizon, mut screen, mut rendering) = (0, 0, 0, 0);
    for mx in &scan.mixins {
        for member in mx.members.iter().filter(|m| !follows(m)) {
            match reach(mx, member) {
                Reach::Hook => hook += 1,
                Reach::Horizon => horizon += 1,
                Reach::Screen => screen += 1,
                Reach::Rendering => rendering += 1,
            }
        }
    }
    let total = hook + horizon + screen + rendering;
    let out = screen + rendering;
    let has_content = !scan.registrations.is_empty() || scan.resources.data() > 0;
    let what = match (rendering > 0, screen > 0) {
        (true, true) => "how Minecraft draws the game and its screens",
        (true, false) => "how Minecraft draws the game",
        _ => "how Minecraft draws its screens",
    };
    // A mod made of patches to the game's internals (a performance mod): each
    // must be rewritten against Horizon's own code, which is its own engine.
    let internals = out == 0 && total >= 5 && hook * 10 <= total && !has_content;
    let (portability, summary) = if internals {
        (
            Portability::Partly,
            format!(
                "Partly portable. {horizon} of its {total} Mixins patch the game's own code, with no Horizon hook for them: each is ported by hand into Horizon's matching Rust function. Horizon is its own engine, so a performance mod's gains may not carry over."
            ),
        )
    } else if total == 0 || out == 0 {
        (
            Portability::Likely,
            if total == 0 {
                "Nothing it changes is out of Horizon's reach.".to_string()
            } else {
                format!(
                    "Its Mixins reach Horizon: {hook} through hooks and {horizon} through Horizon's own functions. Whether the port behaves like the mod depends on translating its code."
                )
            },
        )
    } else if out * 5 >= total * 2 {
        (
            Portability::NotYet,
            format!(
                "Not portable yet. {out} of its {total} Mixins change {what}. Horizon draws with its own renderer: a port can draw its own blocks and screens, but not change how Horizon draws the rest. {}",
                if has_content {
                    "Its blocks, items and data would still carry over, but what the mod is for wouldn't."
                } else {
                    "A port would build, but the mod wouldn't do anything in game."
                }
            ),
        )
    } else {
        (
            Portability::Partly,
            format!(
                "Partly portable. {out} of its {total} Mixins change {what}, which Horizon can't do yet: those features won't carry over. The rest reach Horizon."
            ),
        )
    };
    Outlook {
        hook,
        horizon,
        screen,
        rendering,
        portability,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use porter_model::Side;

    fn member(annotation: &str, method: &str) -> MixinMember {
        MixinMember {
            member: format!("on_{method}"),
            annotation: annotation.into(),
            target_methods: vec![method.into()],
            at: Some("HEAD".into()),
            at_target: None,
            cancellable: false,
            horizon_kind: Some("HEAD".into()),
            verdict: Verdict::Supported("HEAD".into()),
        }
    }

    fn mixin(target: &str, members: Vec<MixinMember>) -> MixinClass {
        MixinClass {
            name: format!("{}Mixin", simple(target)),
            targets: vec![target.into()],
            side: Side::Client,
            added_interfaces: Vec::new(),
            members,
        }
    }

    #[test]
    fn sorts_members_by_what_they_change() {
        let fov = mixin(
            "net.minecraft.client.renderer.GameRenderer",
            vec![member("Inject", "getFov")],
        );
        assert_eq!(reach(&fov, &fov.members[0]), Reach::Hook);
        // GameRenderer.render maps to `frame`, which can't draw anything.
        let render = mixin(
            "net.minecraft.client.renderer.GameRenderer",
            vec![member("Inject", "render")],
        );
        assert_eq!(reach(&render, &render.members[0]), Reach::Rendering);
        let screen = mixin(
            "net.minecraft.client.gui.screens.inventory.AbstractContainerScreen",
            vec![member("Inject", "renderBg")],
        );
        assert_eq!(reach(&screen, &screen.members[0]), Reach::Screen);
        let logic = mixin(
            "net.minecraft.world.entity.LivingEntity",
            vec![member("Inject", "travel")],
        );
        assert_eq!(reach(&logic, &logic.members[0]), Reach::Horizon);
    }

    #[test]
    fn a_mod_that_redraws_screens_is_not_portable_yet() {
        let scan = ModScan {
            mixins: vec![
                mixin(
                    "net.minecraft.client.gui.screens.inventory.AbstractContainerScreen",
                    vec![member("Inject", "render"), member("Inject", "renderBg")],
                ),
                mixin(
                    "net.minecraft.client.renderer.GameRenderer",
                    vec![member("Inject", "render"), member("Shadow", "minecraft")],
                ),
            ],
            ..ModScan::default()
        };
        let o = outlook(&scan);
        assert_eq!(o.portability, Portability::NotYet, "{o:?}");
        assert_eq!(o.total(), 3);
        assert!(o.summary.contains("wouldn't do anything"), "{}", o.summary);
    }

    #[test]
    fn a_mod_of_internal_patches_is_partly_portable() {
        let members = (0..6)
            .map(|i| member("Inject", &format!("internal{i}")))
            .collect();
        let scan = ModScan {
            mixins: vec![mixin("net.minecraft.world.level.chunk.LevelChunk", members)],
            ..ModScan::default()
        };
        let o = outlook(&scan);
        assert_eq!(o.portability, Portability::Partly, "{o:?}");
        assert!(o.summary.contains("performance mod"), "{}", o.summary);
    }

    #[test]
    fn a_zoom_mod_is_likely() {
        let scan = ModScan {
            mixins: vec![mixin(
                "net.minecraft.client.renderer.GameRenderer",
                vec![member("Inject", "getFov")],
            )],
            ..ModScan::default()
        };
        assert_eq!(outlook(&scan).portability, Portability::Likely);
    }
}
