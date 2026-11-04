package com.demo;

import java.util.function.BiConsumer;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.world.level.block.Block;

// The multi-loader pattern: each loader passes its own registration callback.
public class Xplat {
    static final Block ANVIL = null;

    public static void registerBlocks(BiConsumer<Block, ResourceLocation> r) {
        r.accept(ANVIL, id("brass_anvil"));
        r.accept(ANVIL, id("brass_bell"));
    }

    static ResourceLocation id(String path) {
        return ResourceLocation.fromNamespaceAndPath("demo", path);
    }
}
