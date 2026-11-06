package net.shutters;

import net.minecraft.core.registries.Registries;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.neoforged.neoforge.registries.DeferredHolder;
import net.neoforged.neoforge.registries.DeferredRegister;

public class Shutters {
    public static final DeferredRegister<Block> BLOCKS = DeferredRegister.create(Registries.BLOCK, "shutters");

    public static final DeferredHolder<Block, Shutter> OAK_SHUTTER =
        BLOCKS.register("oak_shutter", () -> new Shutter(BlockBehaviour.Properties.of().strength(0.5f)));
    public static final DeferredHolder<Block, Gate> IRON_GATE =
        BLOCKS.register("iron_gate", () -> new Gate(BlockBehaviour.Properties.of().strength(2.0f)));
}
