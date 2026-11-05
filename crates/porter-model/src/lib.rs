//! Shared types: the project model, symbols, verdicts and the state file.
//!
//! Everything the other crates exchange lives here, so the scanner, the translator
//! and the reports agree on one vocabulary. All of it serializes to JSON, which is
//! what `horizon-porter scan --json` writes.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The mod loader a Java mod was written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    Fabric,
    NeoForge,
    Forge,
}

impl fmt::Display for Loader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Loader::Fabric => "Fabric",
            Loader::NeoForge => "NeoForge",
            Loader::Forge => "Forge",
        })
    }
}

/// Where a class a mod refers to comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolOrigin {
    /// Minecraft itself (`net.minecraft.*`, `com.mojang.*`), mapped through Horizon's
    /// port ledger.
    Minecraft,
    /// The loader's API (Fabric API, NeoForge, Forge), mapped to modkit.
    LoaderApi,
    /// The Mixin framework (Sponge Mixin, MixinExtras): its annotations become
    /// horizon-mixins injections, not code.
    MixinFramework,
    /// Libraries that ship with Minecraft (Guava, fastutil, JOML, LWJGL, Gson, Netty,
    /// logging). Most have a Rust counterpart; LWJGL means direct OpenGL calls.
    Platform,
    /// Another mod or library (Registrate, Flywheel...), mapped to a shim if one exists.
    Library,
    /// The mod's own code, translated alongside it.
    Own,
    /// The Java standard library, mapped to Rust's.
    Jdk,
}

impl SymbolOrigin {
    /// Classifies a fully qualified class name. `own` says whether the name is one of
    /// the mod's own classes.
    pub fn classify(fqcn: &str, own: impl Fn(&str) -> bool) -> SymbolOrigin {
        const PREFIXES: &[(&str, SymbolOrigin)] = &[
            ("net.minecraft.", SymbolOrigin::Minecraft),
            ("com.mojang.", SymbolOrigin::Minecraft),
            ("net.fabricmc.", SymbolOrigin::LoaderApi),
            ("net.neoforged.", SymbolOrigin::LoaderApi),
            ("net.minecraftforge.", SymbolOrigin::LoaderApi),
            ("org.spongepowered.asm.", SymbolOrigin::MixinFramework),
            ("com.llamalad7.mixinextras.", SymbolOrigin::MixinFramework),
            // Before "javax.": JSR-305 annotations ship with Minecraft, not the JDK.
            ("javax.annotation.", SymbolOrigin::Platform),
            ("java.", SymbolOrigin::Jdk),
            ("javax.", SymbolOrigin::Jdk),
            ("jdk.", SymbolOrigin::Jdk),
            ("sun.", SymbolOrigin::Jdk),
            ("com.google.common.", SymbolOrigin::Platform),
            ("com.google.gson.", SymbolOrigin::Platform),
            ("it.unimi.dsi.fastutil.", SymbolOrigin::Platform),
            ("org.joml.", SymbolOrigin::Platform),
            ("org.lwjgl.", SymbolOrigin::Platform),
            ("io.netty.", SymbolOrigin::Platform),
            ("org.slf4j.", SymbolOrigin::Platform),
            ("org.apache.logging.", SymbolOrigin::Platform),
            ("org.apache.commons.", SymbolOrigin::Platform),
            ("org.jetbrains.annotations.", SymbolOrigin::Platform),
            ("joptsimple.", SymbolOrigin::Platform),
            ("com.ibm.icu.", SymbolOrigin::Platform),
            ("oshi.", SymbolOrigin::Platform),
        ];
        if own(fqcn) {
            return SymbolOrigin::Own;
        }
        PREFIXES
            .iter()
            .find(|(prefix, _)| fqcn.starts_with(prefix))
            .map(|&(_, origin)| origin)
            .unwrap_or(SymbolOrigin::Library)
    }

    /// A short label for reports.
    pub fn label(self) -> &'static str {
        match self {
            SymbolOrigin::Minecraft => "Minecraft",
            SymbolOrigin::LoaderApi => "loader API",
            SymbolOrigin::MixinFramework => "Mixin framework",
            SymbolOrigin::Platform => "bundled libraries",
            SymbolOrigin::Library => "other mods and libraries",
            SymbolOrigin::Own => "own code",
            SymbolOrigin::Jdk => "Java standard library",
        }
    }
}

/// The verdict for one item of a mod: a class, a Mixin, a registry entry or an asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", content = "note", rename_all = "snake_case")]
pub enum Verdict {
    /// Ported and compiling.
    Ported,
    /// Supported in principle by the target (Horizon, horizon-mixins, modkit); not
    /// yet ported. The note says how.
    Supported(String),
    /// Ported or portable in part; the note says what is missing.
    Partial(String),
    /// Generated as a stub for a human to finish.
    Stubbed(String),
    /// Cannot be ported yet; the note names what is missing (a Horizon hook point,
    /// a horizon-mixins feature, a library shim).
    Blocked(String),
    /// Not needed in Horizon (performance mods, loader glue).
    NotNeeded(String),
}

impl Verdict {
    /// A one-word name for counting and filtering.
    pub fn name(&self) -> &'static str {
        match self {
            Verdict::Ported => "ported",
            Verdict::Supported(_) => "supported",
            Verdict::Partial(_) => "partial",
            Verdict::Stubbed(_) => "stubbed",
            Verdict::Blocked(_) => "blocked",
            Verdict::NotNeeded(_) => "not needed",
        }
    }

    /// The note, if any.
    pub fn note(&self) -> Option<&str> {
        match self {
            Verdict::Ported => None,
            Verdict::Supported(n)
            | Verdict::Partial(n)
            | Verdict::Stubbed(n)
            | Verdict::Blocked(n)
            | Verdict::NotNeeded(n) => Some(n),
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.note() {
            Some(note) => write!(f, "{}: {note}", self.name()),
            None => f.write_str(self.name()),
        }
    }
}

/// Which side of the game code runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// Both, or not known to be one side only.
    #[default]
    Common,
    Client,
    Server,
}

/// What was scanned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    /// A source tree: the mod developer's Gradle project.
    #[default]
    SourceTree,
    /// A built jar. Class-level facts only; Mixin details need the source.
    Jar,
}

/// Everything a scan found in one mod.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModScan {
    /// The path that was scanned.
    pub input: String,
    pub input_kind: InputKind,
    /// Each loader metadata file found. A multi-loader project has several.
    pub metadata: Vec<ModMetadata>,
    pub classes: Vec<ClassInfo>,
    pub mixin_configs: Vec<MixinConfig>,
    pub mixins: Vec<MixinClass>,
    pub registrations: Vec<Registration>,
    pub resources: ResourceSummary,
    /// Jars bundled inside a jar (jar-in-jar), by path.
    pub bundled_jars: Vec<String>,
    /// Client features found in the code (source scans only).
    #[serde(default)]
    pub client: ClientFeatures,
    /// The mod's container menus (`AbstractContainerMenu` subclasses): their slots.
    #[serde(default)]
    pub menus: Vec<MenuInfo>,
    /// The mod's container screens (`AbstractContainerScreen` subclasses).
    #[serde(default)]
    pub screens: Vec<ScreenInfo>,
    /// The mod's block entity renderers.
    #[serde(default)]
    pub renderers: Vec<RendererInfo>,
    /// The mod's own screens that aren't a container's.
    #[serde(default)]
    pub custom_screens: Vec<CustomScreenInfo>,
    /// Fluid tanks and energy buffers of the mod's block entities.
    #[serde(default)]
    pub storages: Vec<StorageInfo>,
    /// Models no block names that the mod bakes to draw as parts
    /// (`ModelEvent.RegisterStandalone`, `PartialModel`): `grind:block/crusher_wheel`.
    #[serde(default)]
    pub standalone_models: Vec<String>,
    /// The network messages the mod defines.
    #[serde(default)]
    pub payloads: Vec<PayloadInfo>,
    /// The config values the mod defines.
    #[serde(default)]
    pub config: Vec<ConfigEntry>,
    /// Problems the scan hit but worked around, and things it could not read.
    pub warnings: Vec<String>,
}

/// The metadata of the mod itself, when a project or jar has several (a mod's own
/// modules, `c2me-base` beside `c2me`; a multi-loader project's loaders).
impl ModScan {
    pub fn main_metadata(&self) -> Option<&ModMetadata> {
        let named: Vec<&ModMetadata> = self.metadata.iter().filter(|m| m.id.is_some()).collect();
        // The id the others extend (`c2me` for `c2me-base`), else the main source
        // set's, else the first.
        let prefix_of_others = named.iter().copied().find(|m| {
            let id = m.id.as_deref().unwrap_or_default();
            named.len() > 1
                && named.iter().all(|o| {
                    let other = o.id.as_deref().unwrap_or_default();
                    other == id
                        || other
                            .strip_prefix(id)
                            .is_some_and(|rest| rest.starts_with(['-', '_', '.']))
                })
        });
        prefix_of_others
            .or_else(|| {
                named
                    .iter()
                    .copied()
                    .find(|m| m.path.replace('\\', "/").contains("src/main/"))
            })
            .or_else(|| named.first().copied())
            .or_else(|| self.metadata.first())
    }
}

/// A container menu: the slots it shows, in the order it adds them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuInfo {
    /// The class, fully qualified.
    pub class: String,
    /// The container's own slots (the player's inventory left out).
    pub slots: Vec<MenuSlot>,
    /// Where the player's inventory starts: its first row's left slot.
    pub inventory: Option<[i32; 2]>,
    /// How many data slots it syncs (`addDataSlots`), when it could be read.
    pub data_slots: usize,
    /// Slots added in a loop the scanner could not unroll: the count is the
    /// container's, not this list's.
    pub approximate: bool,
}

/// One slot of a menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuSlot {
    /// The container's slot it shows.
    pub index: Option<i32>,
    /// Its position in the screen, from the top left.
    pub x: i32,
    pub y: i32,
    /// Players only take from it (a furnace's output).
    pub result: bool,
}

/// A container screen: its size, its texture and the menu it shows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenInfo {
    pub class: String,
    /// The menu class it shows, as written (`CrusherMenu`).
    pub menu: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// Its background texture: `textures/gui/crusher.png`.
    pub texture: Option<String>,
}

/// What a block entity stores besides items: fluid tanks and an energy buffer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageInfo {
    pub class: String,
    /// Each tank's capacity in millibuckets (`new FluidTank(4000)`,
    /// `new FluidStacksResourceHandler(2, 4000)`).
    pub tanks: Vec<i32>,
    /// The energy buffer: capacity, most one insert moves, most one extract moves
    /// (`new SimpleEnergyHandler(10000, 100, 100)`, `new EnergyStorage(...)`).
    pub energy: Option<[i32; 3]>,
}

/// A screen of the mod's own that isn't a container's: a guidebook, a menu opened
/// by a key (`class GuideScreen extends Screen`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomScreenInfo {
    pub class: String,
    /// The classes that open it (`setScreen(new GuideScreen())`).
    pub opened_by: Vec<String>,
}

/// A block entity renderer: `class CrusherRenderer implements
/// BlockEntityRenderer<CrusherBlockEntity, ...>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererInfo {
    pub class: String,
    /// The block entity class it draws, as written (`CrusherBlockEntity`).
    pub entity: String,
}

/// A network message the mod defines: a `CustomPacketPayload` record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayloadInfo {
    pub class: String,
    /// Its channel, `namespace:path`, from its `CustomPacketPayload.Type`.
    pub channel: Option<String>,
    /// The record's components, in order.
    pub fields: Vec<PayloadField>,
    /// Whether clients send it to the server (`playToServer`, `playC2S`).
    pub to_server: bool,
    /// Whether the server sends it to clients (`playToClient`, `playS2C`).
    pub to_client: bool,
    /// The classes that register it, where its handlers are.
    #[serde(default)]
    pub registered_in: Vec<String>,
}

/// One component of a payload record: `BlockPos pos`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayloadField {
    pub name: String,
    /// The Java type as written: `int`, `BlockPos`, `String`.
    pub java_type: String,
}

/// One config value the mod defines with a `ModConfigSpec.Builder`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigEntry {
    /// The class that defines it.
    pub class: String,
    /// Its key, with the sections it is pushed into: `general.maxSpeed`.
    pub path: String,
    pub kind: ConfigKind,
    /// Its default, as written: `true`, `5`, `0.5`, `"text"` unquoted, `FAST`.
    pub default: String,
    pub min: Option<String>,
    pub max: Option<String>,
    /// An enum value's constants, when the enum is the mod's own.
    #[serde(default)]
    pub values: Vec<String>,
    /// Its `comment(...)`.
    pub comment: Option<String>,
}

/// The type of a config value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigKind {
    Bool,
    Int,
    Long,
    Double,
    String,
    Enum,
}

/// One loader metadata file: `fabric.mod.json`, `neoforge.mods.toml` or `mods.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModMetadata {
    pub loader: Loader,
    /// Where it was found, relative to the input.
    pub path: String,
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub license: Option<String>,
    /// The Minecraft version or range the mod declares.
    pub minecraft: Option<String>,
    /// Ids of the mods it depends on (not counting Minecraft, Java and the loader).
    pub dependencies: Vec<String>,
    /// Mixin config files it names.
    pub mixin_configs: Vec<String>,
}

/// The kind of a Java type declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClassKind {
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
}

/// One top-level class (or interface, enum, record) of the mod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassInfo {
    /// Fully qualified name, such as `com.example.block.ShaftBlock`.
    pub name: String,
    pub kind: ClassKind,
    /// Source file or class file, relative to the input.
    pub path: String,
    pub superclass: Option<String>,
    pub interfaces: Vec<String>,
    /// Classes from outside this file it refers to, fully qualified.
    pub references: Vec<String>,
    pub side: Side,
    /// Whether it is a Mixin class.
    pub is_mixin: bool,
    /// Source lines (0 for class files).
    pub lines: usize,
    pub methods: usize,
    /// Its fields, from source scans.
    #[serde(default)]
    pub fields: Vec<FieldInfo>,
    /// The names of its methods, from source scans.
    #[serde(default)]
    pub method_names: Vec<String>,
    /// Values it writes to NBT (`tag.putFloat("Speed", speed)`), from source scans.
    #[serde(default)]
    pub saved: Vec<SavedValue>,
}

/// A field of a class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldInfo {
    pub name: String,
    /// The type as written: `float`, `int`, `List<ItemStack>`.
    pub ty: String,
    pub is_static: bool,
    #[serde(default)]
    pub is_final: bool,
    /// Its initializer as written, when it has one: `70.0f`, `new ArrayList<>()`.
    #[serde(default)]
    pub init: Option<String>,
}

/// A value a class writes to an NBT compound: `tag.putFloat("Speed", speed)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedValue {
    /// The compound key: `Speed`.
    pub key: String,
    /// The put method: `putFloat`, `putInt`, `putBoolean`...
    pub method: String,
    /// The value written, as written: usually a field name.
    pub value: String,
}

/// A Mixin config file (`*.mixins.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixinConfig {
    pub path: String,
    pub package: Option<String>,
    /// Fully qualified Mixin class names, with the side the config lists them under.
    pub mixins: Vec<(String, Side)>,
    pub plugin: Option<String>,
}

/// A Mixin class and what it injects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixinClass {
    pub name: String,
    /// The classes it targets, fully qualified when they could be resolved.
    pub targets: Vec<String>,
    pub side: Side,
    /// Interfaces the Mixin adds to its targets (`implements` on the Mixin class).
    pub added_interfaces: Vec<String>,
    pub members: Vec<MixinMember>,
}

/// One annotated member of a Mixin class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixinMember {
    /// The Java member name in the Mixin class.
    pub member: String,
    /// The annotation, such as `Inject`, `ModifyArg`, `Shadow`, or `merged` for a
    /// plain method added to the target.
    pub annotation: String,
    /// The target methods it names (`method = "tick"`), as written.
    pub target_methods: Vec<String>,
    /// The injection point (`HEAD`, `RETURN`, `INVOKE`...), if any.
    pub at: Option<String>,
    /// The `target` of the injection point, such as the call it selects.
    pub at_target: Option<String>,
    pub cancellable: bool,
    /// The horizon-mixins kind this maps to, if there is one.
    pub horizon_kind: Option<String>,
    pub verdict: Verdict,
}

/// One registry entry the mod adds, as found in its code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registration {
    /// The registry, normalized: `block`, `item`, `block_entity_type`, `entity_type`...
    pub registry: String,
    /// The id without namespace when it could be read, such as `shaft`.
    pub id: Option<String>,
    /// How it was registered: `DeferredRegister`, `Registry.register`, `Registrate`.
    pub via: String,
    /// The class that registers it.
    pub class: String,
    /// The class it creates, as written in the call (`CrankBlock` from
    /// `CrankBlock::new` or `new CrankBlock(...)`), when there is one.
    #[serde(default)]
    pub creates: Option<String>,
    /// The call's arguments as written, to read properties from
    /// (`BlockBehaviour.Properties.ofFullCopy(Blocks.IRON_BLOCK).strength(3f)`).
    #[serde(default)]
    pub args: String,
}

/// Client features the porter can wire to Horizon's hook points.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientFeatures {
    pub keys: Vec<KeyBinding>,
    /// Root command names (`/zoom` → `zoom`), from `dispatcher.register(literal("zoom"))`.
    pub commands: Vec<String>,
}

/// A key binding the mod creates (`new KeyMapping("key.demo.zoom", GLFW_KEY_Z, ...)`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    /// The translation key, which is also the binding's name: `key.demo.zoom`.
    pub name: String,
    /// The default key in vanilla's naming, as Horizon uses it: `key.keyboard.z`.
    pub default_key: String,
    /// The class that creates it.
    pub class: String,
}

/// Counts of resource files: assets and data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceSummary {
    /// Namespaces under `assets/` and `data/`.
    pub namespaces: Vec<String>,
    /// File counts by category, such as `assets/textures` or `data/recipe`.
    pub categories: BTreeMap<String, usize>,
    /// Where the files came from: `src/main/resources`, `src/generated/resources`...
    pub roots: Vec<String>,
}

impl ResourceSummary {
    /// Total files under `assets/`.
    pub fn assets(&self) -> usize {
        self.total("assets/")
    }

    /// Total files under `data/`.
    pub fn data(&self) -> usize {
        self.total("data/")
    }

    fn total(&self, prefix: &str) -> usize {
        self.categories
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(_, n)| n)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_origins() {
        let own = |n: &str| n.starts_with("com.example.");
        assert_eq!(
            SymbolOrigin::classify("net.minecraft.world.level.Level", own),
            SymbolOrigin::Minecraft
        );
        assert_eq!(
            SymbolOrigin::classify("net.neoforged.bus.api.SubscribeEvent", own),
            SymbolOrigin::LoaderApi
        );
        assert_eq!(
            SymbolOrigin::classify("org.spongepowered.asm.mixin.Mixin", own),
            SymbolOrigin::MixinFramework
        );
        assert_eq!(
            SymbolOrigin::classify("com.example.Foo", own),
            SymbolOrigin::Own
        );
        assert_eq!(
            SymbolOrigin::classify("com.tterrag.registrate.Registrate", own),
            SymbolOrigin::Library
        );
        assert_eq!(
            SymbolOrigin::classify("java.util.List", own),
            SymbolOrigin::Jdk
        );
    }

    #[test]
    fn verdict_json_shape() {
        let v = Verdict::Blocked("no TAIL".into());
        assert_eq!(
            serde_json::to_string(&v).unwrap(),
            r#"{"verdict":"blocked","note":"no TAIL"}"#
        );
    }
}
