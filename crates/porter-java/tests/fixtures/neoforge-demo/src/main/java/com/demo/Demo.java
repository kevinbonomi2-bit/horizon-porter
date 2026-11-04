package com.demo;

import com.demo.block.CrankBlock;
import com.demo.block.CrankBlockEntity;
import net.minecraft.core.registries.Registries;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.SoundType;
import net.minecraft.world.level.block.entity.BlockEntityType;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.neoforged.fml.common.Mod;
import net.neoforged.neoforge.registries.DeferredBlock;
import net.neoforged.neoforge.registries.DeferredRegister;
import java.util.function.Supplier;

@Mod(Demo.ID)
public class Demo {
    public static final String ID = "demo";

    public static final DeferredRegister.Blocks BLOCKS = DeferredRegister.createBlocks(ID);
    public static final DeferredRegister.Items ITEMS = DeferredRegister.createItems(ID);
    public static final DeferredRegister<BlockEntityType<?>> BLOCK_ENTITIES =
        DeferredRegister.create(Registries.BLOCK_ENTITY_TYPE, ID);

    public static final DeferredBlock<CrankBlock> CRANK = BLOCKS.register("crank",
        () -> new CrankBlock(BlockBehaviour.Properties.ofFullCopy(Blocks.IRON_BLOCK)
            .strength(3.5f, 6.0f).lightLevel(state -> 7).sound(SoundType.COPPER)));
    public static final DeferredBlock<CrankBlock> CRANK_2 = BLOCKS.registerBlock("crank_2", CrankBlock::new);
    public static final Supplier<BlockEntityType<CrankBlockEntity>> CRANK_BE =
        BLOCK_ENTITIES.register("crank", () -> BlockEntityType.Builder.of(CrankBlockEntity::new, CRANK.get()).build(null));

    static {
        // Not a registration the scanner knows (no id literal, unknown method): skipped.
        ITEMS.registerSimpleBlockItem(CRANK);
        ITEMS.registerSimpleItem("handle");
    }
}
