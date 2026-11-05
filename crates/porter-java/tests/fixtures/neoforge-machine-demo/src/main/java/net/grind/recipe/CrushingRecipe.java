package net.grind.recipe;

import com.mojang.serialization.MapCodec;
import com.mojang.serialization.codecs.RecordCodecBuilder;
import net.grind.Grind;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.crafting.Ingredient;
import net.minecraft.world.level.Level;

/** grind:crushing: one ingredient, one result, and how many ticks it takes. */
public record CrushingRecipe(Ingredient ingredient, ItemStack result, int time) {
    public static CrushingRecipe find(Level level, ItemStack input) {
        return level.getRecipeManager().getAllRecipesFor(Grind.CRUSHING.get()).stream()
            .map(holder -> holder.value())
            .filter(r -> r.ingredient().test(input))
            .findFirst().orElse(null);
    }

    public static class Serializer {
        public static final MapCodec<CrushingRecipe> CODEC = RecordCodecBuilder.mapCodec(i -> i.group(
            Ingredient.CODEC.fieldOf("ingredient").forGetter(CrushingRecipe::ingredient),
            ItemStack.CODEC.fieldOf("result").forGetter(CrushingRecipe::result),
            com.mojang.serialization.Codec.INT.optionalFieldOf("time", 100).forGetter(CrushingRecipe::time)
        ).apply(i, CrushingRecipe::new));
    }
}
