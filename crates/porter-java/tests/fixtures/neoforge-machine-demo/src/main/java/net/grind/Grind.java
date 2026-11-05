package net.grind;

import net.grind.machine.CrusherBlock;
import net.grind.machine.CrusherBlockEntity;
import net.grind.machine.CrusherMenu;
import net.grind.recipe.CrushingRecipe;
import net.minecraft.core.registries.Registries;
import net.minecraft.world.inventory.MenuType;
import net.minecraft.world.item.crafting.RecipeSerializer;
import net.minecraft.world.item.crafting.RecipeType;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntityType;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.neoforged.fml.common.Mod;
import net.neoforged.neoforge.common.extensions.IMenuTypeExtension;
import net.neoforged.neoforge.registries.DeferredBlock;
import net.neoforged.neoforge.registries.DeferredRegister;
import java.util.function.Supplier;

@Mod(Grind.ID)
public class Grind {
    public static final String ID = "grind";

    public static final DeferredRegister.Blocks BLOCKS = DeferredRegister.createBlocks(ID);
    public static final DeferredRegister<BlockEntityType<?>> BLOCK_ENTITIES =
        DeferredRegister.create(Registries.BLOCK_ENTITY_TYPE, ID);
    public static final DeferredRegister<MenuType<?>> MENUS = DeferredRegister.create(Registries.MENU, ID);
    public static final DeferredRegister<RecipeType<?>> RECIPE_TYPES = DeferredRegister.create(Registries.RECIPE_TYPE, ID);
    public static final DeferredRegister<RecipeSerializer<?>> RECIPE_SERIALIZERS =
        DeferredRegister.create(Registries.RECIPE_SERIALIZER, ID);

    public static final DeferredBlock<CrusherBlock> CRUSHER = BLOCKS.register("crusher",
        () -> new CrusherBlock(BlockBehaviour.Properties.ofFullCopy(Blocks.STONE).strength(3.5f)));
    public static final Supplier<BlockEntityType<CrusherBlockEntity>> CRUSHER_BE =
        BLOCK_ENTITIES.register("crusher", () -> BlockEntityType.Builder.of(CrusherBlockEntity::new, CRUSHER.get()).build(null));
    public static final Supplier<MenuType<CrusherMenu>> CRUSHER_MENU =
        MENUS.register("crusher", () -> IMenuTypeExtension.create(CrusherMenu::new));
    public static final Supplier<RecipeType<CrushingRecipe>> CRUSHING =
        RECIPE_TYPES.register("crushing", () -> RecipeType.simple(net.minecraft.resources.ResourceLocation.fromNamespaceAndPath(ID, "crushing")));
    public static final Supplier<RecipeSerializer<CrushingRecipe>> CRUSHING_SERIALIZER =
        RECIPE_SERIALIZERS.register("crushing", CrushingRecipe.Serializer::new);
}
