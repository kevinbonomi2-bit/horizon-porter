# Census: what 150 mods' Java does that the porter doesn't carry over
Written by `tools/census.py` from the mods `tools/survey.py` downloaded, each ported by the current porter (nothing translated or built). A count is how many mods do it, so a fix's reach is its count. The jars stay local (`ai-test/`).

## Vanilla methods mods override that the porter doesn't know
A method the mods' classes declare over a vanilla superclass (a block's `useItemOn`, an item's `inventoryTick`; the decompiler drops `@Override`, so a method that recurs across mods is taken as one). *Not named* counts the mods whose port names it nowhere (no stub, hook or note); *unknown* is when the porter's code doesn't name it at all, so it is dropped without a word. Data generators (run only when the mod is built) are left out. Ranked by unknown, then not named.

### Game logic

| Method | Mods | Not named | Unknown | Mostly on | e.g. |
| --- | --- | --- | --- | --- | --- |
| `stillValid` | 20 | 20 | 20 | `AbstractContainerMenu`, `BlockEntity`, `BaseContainerBlockEntity` | animal_feeding_trough: FeedingTroughMenu |
| `getRenderShape` | 19 | 19 | 19 | `BaseEntityBlock`, `Block`, `HorizontalDirectionalBlock` | animal_feeding_trough: FeedingTroughBlock |
| `quickMoveStack` | 16 | 16 | 16 | `AbstractContainerMenu`, `RecipeBookMenu`, `AbstractCraftingMenu` | animal_feeding_trough: FeedingTroughMenu |
| `appendHoverText` | 15 | 15 | 15 | `Item`, `BlockItem`, `BaseEntityBlock` | advanced-netherite: AdvancedArmorItem |
| `newBlockEntity` | 13 | 13 | 13 | `BaseEntityBlock`, `Block`, `AbstractFurnaceBlock` | animal_feeding_trough: FeedingTroughBlock |
| `mirror` | 12 | 12 | 12 | `Block`, `BaseEntityBlock`, `Stone` | betterend: EndLotusLeafBlock |
| `isPathfindable` | 12 | 12 | 12 | `Block`, `BaseEntityBlock`, `BasePlantBlock` | betterend: MurkweedBlock |
| `onPlace` | 11 | 11 | 11 | `Block`, `TurtleEggBlock`, `BaseEntityBlock` | betterend: MengerSpongeBlock |
| `affectNeighborsAfterRemoval` | 11 | 11 | 11 | `BaseEntityBlock`, `Block`, `BrewingStandBlock` | betternether: BNBrewingStand |
| `codec` | 10 | 10 | 10 | `DefaultFeature`, `LootModifier`, `WoverBiomeData` | advanced-netherite: CropDropsLootModifier |
| `hasAnalogOutputSignal` | 10 | 10 | 10 | `BaseEntityBlock`, `Block`, `Stone` | animal_feeding_trough: FeedingTroughBlock |
| `getAnalogOutputSignal` | 10 | 10 | 10 | `BaseEntityBlock`, `Block`, `ChestBlock` | animal_feeding_trough: FeedingTroughBlock |
| `getDisplayName` | 10 | 10 | 10 | `BlockEntity`, `EnchantingTableBlockEntity`, `BlockItem` | animal_feeding_trough: FeedingTroughBlockEntity |
| `getTicker` | 9 | 9 | 9 | `BaseEntityBlock`, `Block`, `AbstractFurnaceBlock` | animal_feeding_trough: FeedingTroughBlock |
| `setPlacedBy` | 9 | 9 | 9 | `Block`, `DoublePlantBlock`, `VegetationBlock` | betterend: BrimstoneBlock |
| `entityInside` | 9 | 9 | 9 | `Block`, `VegetationBlock`, `BasePlantBlock` | betterend: EndPortalBlock |
| `playerWillDestroy` | 9 | 9 | 9 | `Block`, `VegetationBlock`, `HorizontalDirectionalBlock` | betterend: MengerSpongeWetBlock |
| `getDefaultName` | 9 | 9 | 9 | `BaseContainerBlockEntity`, `RandomizableContainerBlockEntity`, `BlockEntity` | betterend: EndStoneSmelterBlockEntity |
| `preRemoveSideEffects` | 9 | 9 | 9 | `BlockEntity`, `BaseContainerBlockEntity` | betterend: EndStoneSmelterBlockEntity |
| `getUpdatePacket` | 9 | 9 | 9 | `BlockEntity`, `RandomizableContainerBlockEntity`, `ChestBlockEntity` | betterend: FlowerPotBlockEntity |
| `mayPickup` | 9 | 9 | 9 | `Slot`, `ResourceHandlerSlot`, `AbstractContainerMenu` | easy-anvils: ModAnvilMenu |
| `getSerializer` | 8 | 8 | 8 | `BasicDisplay`, `CustomRecipe`, `SimpleSmithingRecipe` | advanced-loot-info: ReiBaseDisplay |
| `getId` | 8 | 8 | 8 | `ListNode`, `BasicDisplay`, `ResourceHandlerSlot` | advanced-loot-info: AlternativesNode |
| `getItems` | 8 | 8 | 8 | `BaseContainerBlockEntity`, `RandomizableContainerBlockEntity`, `BlockEntity` | animal_feeding_trough: FeedingTroughBlockEntity |
| `playerDestroy` | 8 | 8 | 8 | `Block`, `VegetationBlock`, `ChestBlock` | betterend: EmeraldIceBlock |
| `addAdditionalSaveData` | 9 | 8 | 8 | `Animal`, `TamableAnimal`, `StructurePiece` | betterend: BasePiece |
| `canPlaceItem` | 8 | 8 | 8 | `BlockEntity`, `BaseContainerBlockEntity`, `EnchantingTableBlockEntity` | betterend: EndStoneSmelterBlockEntity |
| `slotsChanged` | 8 | 8 | 8 | `AbstractCraftingMenu`, `AnvilMenu`, `AbstractContainerMenu` | curios: CuriosMenu |
| `fallOn` | 7 | 7 | 7 | `Block`, `TurtleEggBlock`, `HorizontalDirectionalBlock` | beautify-refabricated: BookStack |
| `neighborChanged` | 7 | 7 | 7 | `BaseEntityBlock`, `LanternBlock`, `Block` | beautify-refabricated: HangingPot |
| `getCloneItemStack` | 7 | 7 | 7 | `Block`, `VegetationBlock`, `BaseEntityBlock` | beautify-refabricated: HangingPot |
| `getOcclusionShape` | 7 | 7 | 7 | `Block`, `BaseEntityBlock`, `HorizontalDirectionalBlock` | beautify-refabricated: PictureFrame |
| `randomTick` | 8 | 7 | 7 | `Block`, `VegetationBlock`, `TurtleEggBlock` | betterend: AncientEmeraldIceBlock |
| `canSurvive` | 7 | 7 | 7 | `Block`, `GrowingPlantBodyBlock`, `GrowingPlantHeadBlock` | betterend: BlueVineLanternBlock |
| `defineSynchedData` | 7 | 7 | 7 | `Animal`, `TamableAnimal`, `AbstractFish` | betterend: CubozoaEntity |
| `readAdditionalSaveData` | 8 | 7 | 7 | `Animal`, `TamableAnimal`, `AbstractFish` | betterend: CubozoaEntity |
| `setItems` | 7 | 7 | 7 | `RandomizableContainerBlockEntity`, `BaseContainerBlockEntity` | betterend: EndStoneSmelterBlockEntity |
| `getNoItemIcon` | 7 | 7 | 7 | `AbstractContainerMenu`, `AbstractCraftingMenu`, `RecipeBookMenu` | curios: CuriosMenu |
| `findGenerationPoint` | 6 | 6 | 6 | `Structure`, `TemplateStructure` | adorabuild-structures: EndJigsawStructure |
| `canUse` | 6 | 6 | 6 | `Goal`, `MoveToBlockGoal`, `MeleeAttackGoal` | animal_feeding_trough: SelfFeedGoal |
| `canContinueToUse` | 6 | 6 | 6 | `Goal`, `MoveToBlockGoal`, `MeleeAttackGoal` | animal_feeding_trough: SelfFeedGoal |
| `stepOn` | 6 | 6 | 6 | `Block`, `TurtleEggBlock`, `BaseEntityBlock` | betterend: AncientEmeraldIceBlock |
| `registerGoals` | 6 | 6 | 6 | `Animal`, `TamableAnimal`, `AbstractFish` | betterend: DragonflyEntity |
| `onTake` | 8 | 6 | 6 | `Slot`, `ResultSlot`, `ItemCombinerMenu` | betterend: SmelterOutputSlot |
| `clearContent` | 6 | 6 | 6 | `BlockEntity`, `BaseContainerBlockEntity`, `AbstractContainerMenu` | betterend: EndStoneSmelterBlockEntity |

### Client: screens, widgets, renderers, particles

| Method | Mods | Not named | Unknown | Mostly on | e.g. |
| --- | --- | --- | --- | --- | --- |
| `extractRenderState` | 35 | 35 | 35 | `Screen`, `ScreenBase`, `AbstractContainerEventHandler` | advanced-netherite: ConfigScreen |
| `extractWidgetRenderState` | 22 | 22 | 22 | `AbstractWidget`, `EditBox`, `AbstractSliderButton` | azurelib: ColorDisplay |
| `extractContents` | 21 | 21 | 21 | `Button`, `Plain`, `ImageButton` | curios: CosmeticButton |
| `removed` | 20 | 20 | 20 | `Screen`, `AbstractContainerMenu`, `AbstractCraftingMenu` | curios: CuriosMenu |
| `updateWidgetNarration` | 18 | 18 | 18 | `AbstractWidget`, `AbstractButton`, `EditBox` | azurelib: ColorDisplay |
| `mouseDragged` | 15 | 15 | 15 | `Screen`, `AbstractWidget`, `AbstractContainerEventHandler` | advanced-loot-info: ReiScrollWidget |
| `mouseReleased` | 15 | 15 | 15 | `Screen`, `AbstractWidget`, `AbstractContainerEventHandler` | advanced-loot-info: ReiScrollWidget |
| `extractBackground` | 15 | 15 | 15 | `Screen`, `ScreenBase`, `GuiSettings` | betterend: InfusionRecipeScreen |
| `getType` | 14 | 14 | 14 | `BaseEntityBlock`, `SingleQuadParticle`, `EnchantingTableBlockEntity` | advanced-loot-info: ReiTradeDisplay |
| `setFocused` | 12 | 12 | 12 | `AbstractWidget`, `Screen`, `EditBox` | azurelib: ContainerWidget |
| `getLayer` | 12 | 12 | 12 | `SingleQuadParticle`, `RisingParticle` | beautify-refabricated: GlowEssenceParticles |
| `extractContent` | 12 | 12 | 12 | `Screen`, `BaseEntry`, `ListEntry` | emotecraft: HeaderEntry |
| `onClick` | 12 | 11 | 11 | `AbstractWidget`, `ConfigButtonInfo`, `Screen` | azurelib: ColorWidget |
| `charTyped` | 11 | 11 | 11 | `Screen`, `AbstractWidget`, `AbstractContainerEventHandler` | easy-anvils: FormattableEditBox |
| `keyReleased` | 10 | 10 | 10 | `Screen`, `AbstractWidget`, `ScreenBase` | azurelib: ContainerWidget |
| `getHeight` | 10 | 10 | 10 | `AbstractContainerEventHandler`, `AbstractWidget`, `BasicDisplay` | betterend: PedestalBlock |
| `mouseMoved` | 9 | 9 | 9 | `Screen`, `AbstractWidget`, `AbstractContainerEventHandler` | advanced-loot-info: ReiScrollWidget |
| `updateMessage` | 9 | 9 | 9 | `AbstractSliderButton`, `Plain`, `AbstractOptionSliderButton` | azurelib: ColorSlider |
| `applyValue` | 9 | 9 | 9 | `AbstractSliderButton`, `AbstractOptionSliderButton` | azurelib: ColorSlider |
| `getColor` | 9 | 9 | 9 | `AbstractWidget`, `Block`, `TintedParticleLeavesBlock` | betterend: EnderBlock |
| `isPauseScreen` | 9 | 9 | 9 | `Screen` | betterend: InfusionRecipeScreen |
| `getWidth` | 9 | 9 | 9 | `RepeaterUnit`, `AbstractContainerEventHandler`, `AbstractWidget` | enchanting-infuser: ScrollingStringWidget |
| `getName` | 9 | 9 | 9 | `BlockEntity`, `ModelGenerator`, `ShatterConfig` | farmers-delight-refabricated: CookingPotBlockEntity |
| `getLightCoords` | 7 | 7 | 7 | `SingleQuadParticle` | better-combat: SlashParticle |
| `propagatesSkylightDown` | 7 | 7 | 7 | `Block`, `TintedParticleLeavesBlock`, `BaseEntityBlock` | betterend: CrystalMossCoverBlock |
| `repositionElements` | 7 | 7 | 7 | `Screen`, `OptionsSubScreen` | emotecraft: BaseModalScreen |
| `getRowWidth` | 7 | 7 | 7 | `Screen`, `ScreenBase` | emotecraft: KeyBindsMenu |
| `shouldCloseOnEsc` | 7 | 7 | 7 | `Screen`, `KeyframeManagerScreen`, `GenericMessageScreen` | emotecraft: AccountLinkConfirmScreen |
| `isFocused` | 7 | 7 | 7 | `AbstractWidget`, `ScreenBase`, `AbstractContainerEventHandler` | emotecraft: AbstractFastChooseWidget |
| `updateNarration` | 7 | 7 | 7 | `AbstractContainerEventHandler`, `Entry`, `ScrollPanel` | fancymenu: AbstractLayoutEditorWidget |
| `getDescription` | 7 | 7 | 7 | `Item`, `BlockItem`, `ButtonElementBuilder` | fancymenu: VanillaWidgetElementBuilder |
| `setSize` | 6 | 6 | 6 | `AbstractWidget`, `Entity`, `ItemStacksResourceHandler` | betterend: SlimeMixin |
| `isRandomlyTicking` | 6 | 6 | 6 | `Block`, `GrowingPlantHeadBlock`, `TintedParticleLeavesBlock` | betternether: BNLeaves |
| `equals` | 6 | 6 | 6 | `ListEntry`, `SocketAddress`, `CopyOnWriteArrayList` | e4mc: DialtoneAddress |
| `hashCode` | 6 | 6 | 6 | `ListEntry`, `SocketAddress`, `CopyOnWriteArrayList` | e4mc: DialtoneAddress |
| `getMessage` | 6 | 6 | 6 | `AbstractWidget`, `Plain`, `Button` | easy-anvils: FormattingGuideWidget |
| `narrationPriority` | 6 | 6 | 6 | `AbstractContainerEventHandler`, `AbstractWidget`, `ScrollPanel` | fancymenu: AbstractLayoutEditorWidget |
| `narratables` | 6 | 6 | 6 | `Screen`, `ConfigEntry`, `BaseEntry` | farmers-delight-refabricated: FabricConfigListScreen |
| `setSelected` | 5 | 5 | 5 | `ScreenBase`, `Screen`, `AbstractContainerEventHandler` | emotecraft: EmoteSubScreen |
| `scrollBarX` | 5 | 5 | 5 | `Screen`, `ScreenBase` | farmers-delight-refabricated: FabricConfigListScreen |
| `createNarrationMessage` | 5 | 5 | 5 | `AbstractWidget`, `EditBox`, `AbstractSliderButton` | fzzy-config: ChangesWidget |
| `getTexture` | 4 | 4 | 4 | `Widget`, `AbstractScrollWidget`, `AbstractTexture` | advanced-loot-info: JeiScrollWidget |
| `getTooltip` | 4 | 4 | 4 | `ListNode`, `WidgetWithBounds`, `AbstractScrollWidget` | advanced-loot-info: AlternativesNode |
| `isValidClickButton` | 4 | 4 | 4 | `AbstractWidget`, `Screen`, `Button` | azurelib: ColorDisplay |
| `isMouseOver` | 4 | 4 | 4 | `AbstractWidget`, `AbstractContainerEventHandler`, `EditBox` | azurelib: LeftAlignedLabel |

## Game calls no runtime helper names
Calls the mods' game classes (blocks, items, block entities, mobs, menus) make that the porter's helpers (`World`, `Machine`, `Blocks`, `ScreenAt`) don't name in their docs, so a translation of that code may have nothing to call. Some are covered without being named (a position's `getX` is `pos[0]`); generic names (`get`, `stream`) are left out.

| Call | Mods |
| --- | --- |
| `.isClientSide(` | 32 |
| `.getZ(` | 30 |
| `.getX(` | 29 |
| `.getY(` | 28 |
| `.defaultBlockState(` | 27 |
| `.level(` | 24 |
| `.translatable(` | 23 |
| `.registerDefaultState(` | 22 |
| `.getClickedPos(` | 22 |
| `.getItem(` | 20 |
| `.getItemInHand(` | 20 |
| `.getAxis(` | 20 |
| `.getOpposite(` | 19 |
| `.getRandom(` | 19 |
| `.relative(` | 18 |
| `.above(` | 18 |
| `.value(` | 18 |
| `.getHorizontalDirection(` | 17 |
| `.withStyle(` | 17 |
| `.isAlive(` | 16 |
| `.create(` | 15 |
| `.addSlot(` | 14 |
| `.moveItemStackTo(` | 14 |
| `.scheduleTick(` | 14 |
| `.discard(` | 14 |
| `.asItem(` | 13 |
| `.setBlockAndUpdate(` | 13 |
| `.getFirst(` | 13 |
| `.isAir(` | 13 |
| `.levelEvent(` | 13 |
| `.addParticle(` | 13 |
| `.registryAccess(` | 13 |
| `.hasItem(` | 12 |
| `.withSize(` | 12 |
| `.putInt(` | 12 |
| `.destroyBlock(` | 12 |
| `.blockPosition(` | 12 |
| `.position(` | 12 |
| `.clamp(` | 12 |
| `.getSource(` | 11 |
| `.getIntOr(` | 11 |
| `.loadAllItems(` | 11 |
| `.getEntitiesOfClass(` | 11 |
| `.saveAllItems(` | 11 |
| `.awardStat(` | 11 |
| `.nextDouble(` | 11 |
| `.offset(` | 11 |
| `.hasProperty(` | 11 |
| `.getServer(` | 11 |
| `.getYRot(` | 11 |
| `.isCreative(` | 11 |
| `.isHorizontal(` | 11 |
| `.getStepZ(` | 11 |
| `.getStepX(` | 11 |
| `.fromNamespaceAndPath(` | 11 |
| `.setCount(` | 10 |
| `.getUUID(` | 10 |
| `.gameEvent(` | 10 |
| `.north(` | 10 |
| `.west(` | 10 |
| `.east(` | 10 |
| `.setDeltaMovement(` | 10 |
| `.south(` | 10 |
| `.setReturnValue(` | 10 |
| `.sendBlockUpdated(` | 10 |
| `.getRotation(` | 9 |
| `.isEmptyBlock(` | 9 |
| `.playLocalSound(` | 9 |
| `.damageSources(` | 9 |
| `.sendOverlayMessage(` | 9 |
| `.getMainHandItem(` | 9 |
| `.cancel(` | 9 |
| `.atCenterOf(` | 9 |
| `.saveWithoutMetadata(` | 9 |
| `.isSpectator(` | 9 |
| `.move(` | 9 |
| `.getHoverName(` | 9 |
| `.putBoolean(` | 9 |
| `.builtInRegistryHolder(` | 8 |
| `.floor(` | 8 |

## Block and item settings the content mod doesn't read
`Properties` settings (`.strength(..)`, `.craftRemainder(..)`) that appear in the mods and nowhere in porter-content or porter-java.

| Setting | Mods |
| --- | --- |
| `.mapColor(` | 12 |
| `.pushReaction(` | 8 |
| `.forceSolidOn(` | 6 |
| `.useBlockDescriptionPrefix(` | 5 |
| `.instrument(` | 5 |
| `.ignitedByLava(` | 5 |
| `.randomTicks(` | 5 |
| `.instabreak(` | 5 |
| `.noCollision(` | 4 |
| `.stream(` | 4 |
| `.component(` | 3 |
| `.jukeboxPlayable(` | 3 |
| `.offsetType(` | 2 |
| `.isRedstoneConductor(` | 2 |
| `.overrideLootTable(` | 2 |
| `.isSuffocating(` | 2 |
| `.cookingFuel(` | 2 |
| `.isViewBlocking(` | 2 |
| `.replaceable(` | 2 |
| `.spawnEgg(` | 2 |
| `.keySet(` | 2 |
| `.containsKey(` | 2 |
| `.set(` | 2 |
| `.setHeaderComments(` | 2 |

## Block entity fields with no Rust type

| Java type | Mods |
| --- | --- |
| `NonNullList` | 8 |
| `Component` | 4 |
| `ContainerData` | 3 |
| `UUID` | 3 |
| `List` | 3 |
| `CachedCheck` | 2 |
| `Optional` | 2 |
| `ContainerOpenersCounter` | 2 |
| `Reference2IntOpenHashMap` | 1 |
| `RecipeHolder` | 1 |
| `EternalRitual` | 1 |
| `InfusionRitual` | 1 |
| `BlockState` | 1 |
| `LockCode` | 1 |
| `ItemStackHandler` | 1 |
| `CookingPotItemHandler` | 1 |
| `Object2IntOpenHashMap` | 1 |
| `Death` | 1 |
| `SimpleContainer` | 1 |
| `BlockPos` | 1 |
| `IBackpackWrapper` | 1 |
| `ResourceHandler` | 1 |
| `EnergyHandler` | 1 |
| `MultiInventoryAccess` | 1 |
| `Map` | 1 |

## Horizon hooks left to wire by hand

None: every Horizon hook the mods' code maps to is wired by the porter.

## What translating them would cost

`translate --dry-run` (no model called) for 113 mods: 1488 functions, about $4.55 with Haiku for one try each (fixes add more).

| Mod | Functions | About |
| --- | --- | --- |
| fancymenu | 109 | $0.40 |
| farmers-delight-refabricated | 75 | $0.37 |
| betternether | 63 | $0.22 |
| betterend | 62 | $0.24 |
| waystones | 58 | $0.26 |
| toms-storage | 50 | $0.21 |
| sophisticated-core | 41 | $0.18 |
| sophisticated-backpacks | 39 | $0.23 |
| travelersbackpack | 38 | $0.23 |
| jade | 35 | $0.11 |
| carpet | 34 | $0.09 |
| iris | 32 | $0.07 |
| biomes-o-plenty | 29 | $0.09 |
| curios | 27 | $0.08 |
| macaws-windows | 26 | $0.12 |
| emotecraft | 25 | $0.05 |
| open-parties-and-claims | 25 | $0.09 |
| azurelib | 23 | $0.04 |
| jei | 22 | $0.06 |
| beautify-refabricated | 21 | $0.06 |

## Gaps the translations noted (trial ports)

What Haiku wrote it couldn't do in the 8 translated trial mods, as it said it: each names a helper or hook the porter lacks.

| Note | Times | Mods |
| --- | --- | --- |
| Missing: `player.swing(hand, ..)`. | 4 | 1 |
| Missing: `player.swing(hand, SwingAnimation.DEFAULT, true)`. | 3 | 1 |
| Missing: the BLOCK_CHANGE game event and the client-side CROP_PLANTED sound. | 3 | 1 |
| Missing: Java also XORs in `player.getUUID().hashCode() << 32` when a player is | 2 | 1 |
| present. `Placement` carries no player or UUID, so that step cannot be done here. | 2 | 1 |
| Missing: `player.awardStat(Stats.POT_FLOWER)`. | 2 | 1 |
| Missing: the return type here cannot schedule a fluid tick, and `Blocks` has no tick call, so the | 2 | 1 |
| Missing: this return type cannot schedule a fluid tick, and `Blocks` has no tick call, so the | 2 | 1 |
| Java: extractBackground(graphics, mouseX, mouseY, ticks). Not reachable here: | 1 | 1 |
| Java: super.extractRenderState(...). Not reachable here: the buttons are widgets | 1 | 1 |
| Java: graphics.text(font, this.title, width / 2, 7, 0xFFFFFF). Not reachable here: | 1 | 1 |
| getDescriptionTop() and getDescriptionBottom(). Not reachable here: the translated | 1 | 1 |
| vanilla Screen.mouseClicked, which passes it to the widgets. Not reachable here: | 1 | 1 |
| function is not given, so their hit-testing and onPress actions cannot run. | 1 | 1 |
| Screen.keyPressed. Not reachable here: the widgets it passes keys to (a focused | 1 | 1 |
| are not reachable here, and none of them takes scrolling, so the result is false. | 1 | 1 |
| cannot be derived here. Missing: the BeautifyConfig field list and AutoConfig's | 1 | 1 |
| Missing: `canSupportCenter` needs a block shape query, which `Blocks` does not offer. | 1 | 1 |
| `BeautifyConfig` is not reachable from this module, so the Java defaults stand in | 1 | 1 |
| for `searchRadius` (6) and `opensFromRoot` (true). Missing: reading the saved config. | 1 | 1 |
| other properties. Missing: Horizon sets properties one at a time, so this | 1 | 1 |
| Missing: `isFaceSturdy(level, below, UP)` needs a block shape query, which Horizon's | 1 | 1 |
| Missing: `levelEvent(1505)` particles, and shrinking the held stack | 1 | 1 |
| Missing: spawning the flower as an ItemEntity at `pos`; `World` has no spawn call. | 1 | 1 |
| Missing: `player.setItemInHand` giving 2 flowers if grown, else 1. | 1 | 1 |
| Missing: growing the held stack by 2 if grown, else 1. | 1 | 1 |
| Missing: shrinking the held stack (unless in creative). | 1 | 1 |
| Missing: `level.gameEvent(null, GameEvent.BLOCK_CHANGE, pos)`, which `World` cannot send. | 1 | 1 |
| Missing: `player.setItemInHand(hand, new ItemStack(GLOWSTONE_DUST, currentLevel / 5))`. | 1 | 1 |
| Missing: `playerStack.consume(1, player)`, shrinking the held stack. Horizon's | 1 | 1 |
| Missing: Horizon's `Blocks` does not give the held item's max stack size. | 1 | 1 |
| Missing: `World` cannot set the hand; `give` puts the flower in the inventory instead. | 1 | 1 |
| Missing: `World` cannot grow the held stack; `give` adds the flower to the inventory instead. | 1 | 1 |
| Missing: `Blocks` has no survival query, and the return type cannot schedule a tick. The | 1 | 1 |
| `getFileName()` are not in the given module, so the labels cannot be | 1 | 1 |
| item slots and `Stove` holds no fields, so those parts cannot be ported here. | 1 | 1 |
| not given, so those particles cannot be placed and are not sent here. | 1 | 1 |
| Missing: `tryTransfer` needs the block's FACING (for the collection | 1 | 1 |
| that block and the second-level heat-conductor check are not reachable through | 1 | 1 |
| `super.extractBackground` draws the recipe book's own background. That is not reachable | 1 | 1 |
